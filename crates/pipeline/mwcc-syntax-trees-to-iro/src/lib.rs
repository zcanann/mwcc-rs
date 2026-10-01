//! Build the typed IRO representation of a function from its syntax tree,
//! then run the IRO passes (see [`passes`]).
//!
//! The builder resolves every expression's type, makes pointer arithmetic
//! explicit (indices are scaled to bytes), turns member, index and
//! dereference accesses into loads and stores through `base + index +
//! offset`, and turns guards and the final return into statements. It
//! rejects (with a diagnostic) anything the PCode pipeline does not model.

pub mod passes;
mod strength;
mod unroll;

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_iro::{
    element_size, is_float, is_general_word, is_narrow, is_unsigned, is_value_type, is_wide, pointee_type, pointer_to, promote,
    BinaryOp, Expr, ExprKind,
    Function, Idiom, IntrinsicOp, Place, Stmt, Type, UnaryOp, Unit, VarId, Variable, VariableKind,
};
use mwcc_syntax_trees as ast;
use mwcc_syntax_trees::{BinaryOperator, Expression, Statement, UnaryOperator};

const ARGUMENT_REGISTERS: usize = 8;

pub fn unsupported(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("PCode lowering: {} (not yet supported)", what.into()))
}

/// The built function, plus facts the lowering needs about its source form.
pub struct Built {
    pub function: Function,
    /// The source returns from more than its final expression (guards,
    /// `return` statements, a conditional final value): every return goes
    /// through one return variable, as MWCC's single return point does.
    pub returns_through_variable: bool,
}

/// Build the IR for `function` and run the IRO passes.
pub fn build(function: &ast::Function, unit: &Unit<'_>) -> Compilation<Built> {
    let mut built = build_unoptimized(function, unit)?;
    if std::env::var_os("MWCC_IRO_NO_SCALARIZE").is_none() {
        passes::scalarize(&mut built.function, unit.keeps_struct_stores);
    }
    passes::run(&mut built.function, unit.branch_preserving, unit.reassociates_sums, unit.unrolling);
    // (Without an explicit speed goal, -O3/-O4 count in CTR unrolled.)
    if unit.strength_reduction && !unit.branch_preserving && std::env::var_os("MWCC_IRO_NO_PARTIAL_UNROLL").is_none() {
        unroll::unroll_partially(&mut built.function, unit.unrolling);
        passes::for_each_expression(&mut built.function.body, &mut |expression| passes::fold(expression));
        passes::displacements(&mut built.function.body);
    }
    if unit.strength_reduction && std::env::var_os("MWCC_IRO_NO_CONSTANT_PROPAGATION").is_none() {
        strength::propagate_constants(&mut built.function);
    }
    if unit.strength_reduction && std::env::var_os("MWCC_IRO_NO_STRENGTH_REDUCTION").is_none() {
        strength::strength_reduce(&mut built.function, !unit.branch_preserving, unit.unrolling);
        strength::remove_dead_inductions(&mut built.function);
        strength::remove_dead_counted_updates(&mut built.function);
    }
    if unit.branch_preserving && std::env::var_os("MWCC_IRO_NO_UNINDEXED").is_none() {
        let absolute = |name: &str| unit.globals.get(name).is_some_and(|global| !global.small_data);
        passes::unindexed_absolute(&mut built.function.body, &absolute);
    }
    Ok(built)
}

/// Build the IR for an unoptimized (`-O0`) compile: no IRO passes beyond the
/// front end's literal folding, and every `return` leaves directly.
pub fn build_unoptimized_compile(function: &ast::Function, unit: &Unit<'_>) -> Compilation<Built> {
    let mut built = build_unoptimized(function, unit)?;
    passes::run_unoptimized(&mut built.function);
    built.returns_through_variable = false;
    Ok(built)
}

/// Build the IR without running the IRO passes.
pub fn build_unoptimized(function: &ast::Function, unit: &Unit<'_>) -> Compilation<Built> {
    if std::env::var("MWCC_SYNTAX_DUMP").is_ok_and(|name| name == function.name) {
        eprintln!("{:#?}", function.statements);
    }
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return Err(unsupported("inline assembly"));
    }
    if function.return_type != Type::Void && !is_value_type(function.return_type) && !is_wide(function.return_type) {
        return Err(unsupported(format!("return type {:?}", function.return_type)));
    }
    let mut variables = Vec::new();
    for parameter in &function.parameters {
        if !is_value_type(parameter.parameter_type) && !is_wide(parameter.parameter_type) {
            return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
        }
        variables.push(Variable {
            name: parameter.name.clone(),
            ty: parameter.parameter_type,
            kind: VariableKind::Parameter,
            frame: None,
            initialized: false,
            raw: false,
        volatile: false,
        });
    }
    let floats = function.parameters.iter().filter(|p| is_float(p.parameter_type)).count();
    if function.parameters.len() - floats > ARGUMENT_REGISTERS || floats > ARGUMENT_REGISTERS {
        return Err(unsupported("stack-passed parameters"));
    }
    let taken = addresses_taken(function);
    // -O0: a local only initialized at its declaration or assigned (never
    // read) is no register variable: it lives in the frame.
    let mentioned = if unit.unoptimized && std::env::var_os("MWCC_IRO_O0_NO_INIT_ONLY_FRAME").is_none() {
        format!(
            "{:?}{:?}{:?}{:?}",
            function.statements,
            function.guards,
            function.return_expression,
            function.locals.iter().map(|local| &local.initializer).collect::<Vec<_>>()
        )
    } else {
        String::new()
    };
    let initialized_only = |local: &ast::LocalDeclaration| {
        unit.unoptimized
            // (Or one assigned exactly once and never read: its store stays.)
            && (local.initializer.is_some()
                || std::env::var_os("MWCC_IRO_O0_WRITE_ONLY_REGISTERS").is_none()
                    && mentioned.matches(&format!("Assign {{ name: {:?}", local.name)).count() == 1)
            && local.array_length.is_none()
            && !mentioned.is_empty()
            && !mentioned.contains(&format!("Variable({:?})", local.name))
    };
    // A `static const` scalar local with a literal initializer (an inlined
    // `sqrtf`'s `_half`) is its constant.
    let mut folded: HashMap<String, Expr> = HashMap::new();
    for local in &function.locals {
        if local.is_static {
            let value = (local.is_const && local.array_length.is_none())
                .then(|| local.initializer.as_ref().and_then(|value| literal_value(value, local.declared_type)))
                .flatten();
            match value {
                Some(value) => {
                    folded.insert(local.name.clone(), value);
                    continue;
                }
                None => return Err(unsupported("static or volatile locals")),
            }
        }
        // Arrays, structs and scalars whose address is taken live in the frame.
        let element = match local.declared_type {
            Type::Struct { size, align } => Some((size, u32::from(align).max(1))),
            ty if is_value_type(ty) || is_wide(ty) => Some((mwcc_iro::width(ty), mwcc_iro::width(ty))),
            _ => None,
        };
        let frame = match (local.array_length, element) {
            (Some(length), Some((size, align))) => Some((size * u32::from(length), align)),
            (None, Some(object))
                if matches!(local.declared_type, Type::Struct { .. })
                    || taken.contains(&local.name)
                    || initialized_only(local)
                    || local.is_volatile =>
            {
                Some(object)
            }
            (None, Some(_)) => None,
            _ => return Err(unsupported(format!("local type {:?}", local.declared_type))),
        };
        // An initialized frame array copies a constant image (its pointer
        // elements would need relocations).
        if frame.is_some()
            && local.data_bytes.is_some()
            && (!local.data_relocations.is_empty()
                || local.array_length.is_none()
                || std::env::var_os("MWCC_IRO_NO_IMAGES").is_some())
        {
            return Err(unsupported("an initialized frame array"));
        }
        variables.push(Variable {
            name: local.name.clone(),
            ty: local.declared_type,
            kind: VariableKind::Local,
            frame,
            initialized: local.initializer.is_some(),
            raw: false,
            volatile: local.is_volatile,
        });
    }
    // A parameter whose address is taken lives in a frame slot (below the
    // locals' slots) that the incoming register is stored to on entry; the
    // register itself becomes a hidden parameter.
    let mut parameter_homes: Vec<(VarId, VarId)> = Vec::new();
    if function.parameters.iter().any(|parameter| taken.contains(&parameter.name)) {
        if unit.unoptimized || unit.early_frame {
            return Err(unsupported("address of a parameter"));
        }
        for (index, parameter) in function.parameters.iter().enumerate().rev() {
            if !taken.contains(&parameter.name) {
                continue;
            }
            let width = mwcc_iro::width(parameter.parameter_type);
            variables[index].name = format!("{}$in", parameter.name);
            parameter_homes.push((index, variables.len()));
            variables.push(Variable {
                name: parameter.name.clone(),
                ty: parameter.parameter_type,
                kind: VariableKind::Local,
                frame: Some((width, width)),
                initialized: false,
            raw: false,
            volatile: false,
            });
        }
    }
    let arrays = function
        .locals
        .iter()
        .filter(|local| local.array_length.is_some())
        .filter_map(|local| variables.iter().position(|variable| variable.name == local.name && variable.kind == VariableKind::Local))
        .collect();
    // Multi-dimensional local arrays: the byte stride of one row.
    let rows = function
        .locals
        .iter()
        .filter_map(|local| {
            let row = u32::from(local.row_bytes?);
            let id = variables.iter().position(|variable| variable.name == local.name && variable.kind == VariableKind::Local)?;
            Some((id, row))
        })
        .collect();
    let mut builder = Builder {
        arrays,
        rows,
        folded,
        return_type: function.return_type,
        function_name: &function.name,
        unit,
        names: variables.iter().enumerate().map(|(id, variable)| (variable.name.clone(), id)).collect(),
        variables: &variables,
        pending: Vec::new(),
        post: Vec::new(),
        strings: Vec::new(),
        guarded: 0,
        temporaries: Vec::new(),
        direct_return: false,
    };
    let mut body = Vec::new();
    for &(incoming, home) in parameter_homes.iter().rev() {
        let ty = builder.variables[home].ty;
        body.push(Stmt::Store {
            place: Place::Memory { base: Box::new(builder.local_address(home)), index: None, offset: 0 },
            ty,
            value: Expr { kind: ExprKind::Var(incoming), ty },
        });
    }
    let mut images: Vec<Vec<u8>> = Vec::new();
    for local in &function.locals {
        // (A folded static has no variable and no runtime initialization.)
        if builder.folded.contains_key(&local.name) {
            continue;
        }
        if let (Some(bytes), Some((size, align))) = (&local.data_bytes, builder.names.get(&local.name).and_then(|&id| builder.variables[id].frame)) {
            // The image is the array's bytes (zero-padded; a string exactly the
            // array's length drops its NUL).
            let variable = builder.names[&local.name];
            let mut image = bytes.clone();
            image.resize(size as usize, 0);
            images.push(image);
            let ty = Type::Struct { size, align: u8::try_from(align).unwrap_or(4) };
            let source = Expr { kind: ExprKind::Image(images.len() - 1), ty: Type::StructPointer { element_size: size } };
            body.push(Stmt::Store {
                place: Place::Memory { base: Box::new(builder.local_address(variable)), index: None, offset: 0 },
                ty,
                value: Expr { kind: ExprKind::Load { base: Box::new(source), index: None, offset: 0 }, ty },
            });
            continue;
        }
        // A struct-typed local initialized from another object copies it.
        if let (Some(initializer), Type::Struct { .. }) = (&local.initializer, local.declared_type) {
            if local.array_length.is_none() && std::env::var_os("MWCC_IRO_NO_STRUCT_COPY").is_none() {
                body.extend(builder.assignment(&Expression::Variable(local.name.clone()), initializer)?);
                body.append(&mut builder.pending);
                continue;
            }
        }
        if let Some(initializer) = &local.initializer {
            let variable = builder.names[&local.name];
            let value = assigned(builder.expression(initializer)?, local.declared_type);
            body.append(&mut builder.pending);
            body.push(match builder.variables[variable].frame {
                Some(_) if local.array_length.is_none() && is_value_type(local.declared_type) => Stmt::Store {
                    place: Place::Memory { base: Box::new(builder.local_address(variable)), index: None, offset: 0 },
                    ty: local.declared_type,
                    value,
                },
                Some(_) => return Err(unsupported("an initialized frame aggregate")),
                None => Stmt::Assign { variable, value },
            });
            body.append(&mut builder.post);
        }
    }
    for statement in &function.statements {
        body.extend(builder.statement(statement)?);
    }
    for guard in &function.guards {
        let mut condition = promoted(builder.expression(&guard.condition)?);
        body.append(&mut builder.pending);
        // (Steps in the condition run before the branch; the tested value is
        // copied first when they change it.)
        if !builder.post.is_empty() {
            let after = std::mem::take(&mut builder.post);
            condition = builder.frozen(condition, &after, &mut body);
            body.extend(after);
        }
        let value = builder.returned(&guard.value)?;
        let then_body: Vec<Stmt> = builder.pending.drain(..).chain([Stmt::Return(Some(value))]).collect();
        body.push(Stmt::If { condition, then_body, else_body: Vec::new() });
    }
    if let Some(value) = &function.return_expression {
        // The final value falls through to the exit.
        let value = builder.returned(value)?;
        body.append(&mut builder.pending);
        body.push(Stmt::SetReturn(value));
        // A register variable's step after the final value is dead.
        builder.post.clear();
    }
    let temporaries = std::mem::take(&mut builder.temporaries);
    let mut builder_strings = std::mem::take(&mut builder.strings);
    let mut variables = variables;
    variables.extend(temporaries);
    let returns_through_variable = function.return_type != Type::Void
        && (!function.guards.is_empty()
            || has_return(&function.statements)
            || matches!(function.return_expression, Some(Expression::Conditional { .. })));
    Ok(Built {
        function: Function {
            strings: std::mem::take(&mut builder_strings),
            images,
            name: function.name.clone(),
            return_type: function.return_type,
            parameter_count: function.parameters.len(),
            variables,
            body,
        },
        returns_through_variable,
    })
}

/// An lvalue whose address has no side effects (evaluating it twice is
/// harmless).
fn pure_lvalue(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(_) | Expression::IntegerLiteral(_) => true,
        Expression::Dereference { pointer } => pure_lvalue(pointer),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => pure_lvalue(base),
        Expression::Index { base, index } => pure_lvalue(base) && pure_lvalue(index),
        Expression::Cast { operand, .. } => pure_lvalue(operand),
        Expression::Binary { left, right, .. } => pure_lvalue(left) && pure_lvalue(right),
        _ => false,
    }
}

/// A call evaluated for its effects (an expanded void call leaves nothing).
fn evaluated(call: Expr) -> Vec<Stmt> {
    if call.ty == Type::Void && matches!(call.kind, ExprKind::Int(_)) {
        Vec::new()
    } else {
        vec![Stmt::Eval(call)]
    }
}

/// Whether any statement assigns `variable`.
fn body_assigns(body: &[Stmt], variable: VarId) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Assign { variable: assigned, .. } => *assigned == variable,
        Stmt::If { then_body, else_body, .. } => body_assigns(then_body, variable) || body_assigns(else_body, variable),
        Stmt::Loop { body, step, effects, .. } => body_assigns(body, variable) || body_assigns(step, variable) || body_assigns(effects, variable),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| body_assigns(arm, variable)),
        _ => false,
    })
}

/// Whether a return sits inside a loop or switch (its `break` would not
/// leave the expansion).
fn returns_in_breakable(body: &[Stmt]) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::If { then_body, else_body, .. } => returns_in_breakable(then_body) || returns_in_breakable(else_body),
        Stmt::Loop { body, step, effects, .. } => returns_anywhere(body) || returns_anywhere(step) || returns_anywhere(effects),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| returns_anywhere(arm)),
        _ => false,
    })
}

/// An expansion's returns as result assignments and `break`s out of the
/// enclosing `do ... while (0)`.
fn early_returns(body: Vec<Stmt>, result: Option<VarId>, ty: Type) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(body.len());
    for statement in body {
        match statement {
            Stmt::Return(value) => {
                if let (Some(id), Some(value)) = (result, value) {
                    out.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
                }
                out.push(Stmt::Break);
            }
            Stmt::If { condition, then_body, else_body } => out.push(Stmt::If {
                condition,
                then_body: early_returns(then_body, result, ty),
                else_body: early_returns(else_body, result, ty),
            }),
            other => out.push(other),
        }
    }
    out
}

/// Whether any statement returns (or sets the return value).
fn returns_anywhere(body: &[Stmt]) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Return(_) | Stmt::SetReturn(_) => true,
        Stmt::If { then_body, else_body, .. } => returns_anywhere(then_body) || returns_anywhere(else_body),
        Stmt::Loop { body, step, effects, .. } => returns_anywhere(body) || returns_anywhere(step) || returns_anywhere(effects),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| returns_anywhere(arm)),
        _ => false,
    })
}

fn has_return(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Return(_) => true,
        Statement::Switch { arms, default, .. } => arms
            .iter()
            .map(|arm| &arm.body)
            .chain(default.iter())
            .any(|body| match body {
                ast::ArmBody::Return(_) => true,
                ast::ArmBody::Statements(statements) => has_return(statements),
            }),
        Statement::If { then_body, else_body, .. } => has_return(then_body) || has_return(else_body),
        Statement::Loop { body, .. } => has_return(body),
        _ => false,
    })
}

struct Builder<'a, 'u> {
    /// Frame variables that are arrays.
    arrays: Vec<VarId>,
    /// Multi-dimensional frame arrays: variable -> row bytes.
    rows: HashMap<VarId, u32>,
    /// Folded `static const` scalar locals: name -> value.
    folded: HashMap<String, Expr>,
    return_type: Type,
    /// The function's name (its pointer variables' signatures are keyed by it).
    function_name: &'a str,
    unit: &'a Unit<'u>,
    names: HashMap<String, VarId>,
    variables: &'a [Variable],
    /// Assignments inside the expression being built, hoisted before the
    /// statement that contains it.
    pending: Vec<Stmt>,
    /// Post-increments inside the expression being built, applied after the
    /// statement that contains it.
    post: Vec<Stmt>,
    /// Inside a conditionally or repeatedly evaluated operand, where an
    /// assignment cannot be hoisted.
    guarded: usize,
    /// Registers holding assigned values, numbered after `variables`.
    temporaries: Vec<Variable>,
    /// String literals by bytes, in first-use order.
    strings: Vec<Vec<u8>>,
    /// The call being built is the returned value itself.
    direct_return: bool,
}

impl Builder<'_, '_> {
    fn statements(&mut self, statements: &[Statement]) -> Compilation<Vec<Stmt>> {
        let mut out = Vec::new();
        for statement in statements {
            out.extend(self.statement(statement)?);
        }
        Ok(out)
    }

    /// An expression evaluated for its effects, as statements (preceded by
    /// the assignments hoisted out of it).
    fn effects(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        let outer = std::mem::take(&mut self.pending);
        let outer_post = std::mem::take(&mut self.post);
        let result = self.effects_inner(expression);
        let hoisted = std::mem::replace(&mut self.pending, outer);
        let after = std::mem::replace(&mut self.post, outer_post);
        let mut out = result?;
        if !hoisted.is_empty() {
            out.splice(0..0, hoisted);
        }
        out.extend(after);
        Ok(out)
    }

    /// A tested value read before `after` (post-steps) runs: the operands
    /// they change are copied first into `before`, a narrow one kept raw
    /// (extended at the test).
    fn frozen(&mut self, value: Expr, after: &[Stmt], before: &mut Vec<Stmt>) -> Expr {
        let changed = (0..self.variables.len() + self.temporaries.len())
            .any(|id| body_assigns(after, id) && value.mentions(id));
        if !changed {
            return value;
        }
        match value.kind {
            ExprKind::Binary(op, left, right) if op.is_comparison() => {
                let left = self.frozen(*left, after, before);
                let right = self.frozen(*right, after, before);
                Expr::binary(op, left, right, value.ty)
            }
            ExprKind::Unary(UnaryOp::LogicalNot, operand) => {
                let operand = self.frozen(*operand, after, before);
                Expr::unary(UnaryOp::LogicalNot, operand, value.ty)
            }
            kind => {
                let value = Expr { kind, ty: value.ty };
                let (inner, ty) = match &value.kind {
                    ExprKind::Convert(inner) if is_narrow(inner.ty) => ((**inner).clone(), value.ty),
                    _ => (value.clone(), value.ty),
                };
                let id = self.temporary(inner.ty);
                let local = id - self.variables.len();
                self.temporaries[local].raw = is_narrow(inner.ty);
                let copy = Expr { kind: ExprKind::Var(id), ty: inner.ty };
                before.push(Stmt::Assign { variable: id, value: inner.clone() });
                if inner.ty == ty { copy } else { Expr { kind: ExprKind::Convert(Box::new(copy)), ty } }
            }
        }
    }

    /// An operand evaluated on its own path: its value, and the statements
    /// that must run before it is used (its effects, then its post-steps,
    /// the value copied first where they change it).
    fn operand_with_effects(&mut self, operand: &Expression) -> Compilation<(Expr, Vec<Stmt>)> {
        let outer = std::mem::take(&mut self.pending);
        let outer_post = std::mem::take(&mut self.post);
        let value = self.expression(operand);
        let effects = std::mem::replace(&mut self.pending, outer);
        let after = std::mem::replace(&mut self.post, outer_post);
        let value = value?;
        // The value is read before the post-steps: copy what they change.
        let mut before = effects;
        let value = self.frozen(value, &after, &mut before);
        before.extend(after);
        Ok((value, before))
    }

    /// A discarded operand's effects; one without effects (a value) is none.
    fn effects_or_nothing(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        match expression {
            Expression::IntegerLiteral(_) | Expression::Variable(_) => Ok(Vec::new()),
            Expression::Cast { operand, .. } if matches!(operand.as_ref(), Expression::IntegerLiteral(_) | Expression::Variable(_)) => {
                Ok(Vec::new())
            }
            other => self.effects(other),
        }
    }

    fn effects_inner(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        Ok(match expression {
            Expression::Call { name, arguments } => evaluated(self.call(name, arguments, true)?),
            Expression::CallThrough { target, arguments, .. } => vec![Stmt::Eval(self.indirect_call(target, arguments, true, None)?)],
            Expression::Cast { target_type: Type::Void, operand } => match operand.as_ref() {
                Expression::Call { name, arguments } => evaluated(self.call(name, arguments, true)?),
                // A discarded variable still counts as a reference (-O0
                // register-variable ranking).
                Expression::Variable(name) => match self.names.get(name) {
                    Some(&id) => vec![Stmt::Eval(Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty })],
                    None => Vec::new(),
                },
                Expression::IntegerLiteral(_) => Vec::new(),
                other => return Err(unsupported(format!("statement expression void {}", expression_name(other)))),
            },
            Expression::Assign { target, value } => self.assignment(target, value)?,
            Expression::PostStep { target, operator, pointer_link: None } => {
                let step = Expression::Binary {
                    operator: *operator,
                    left: target.clone(),
                    right: Box::new(Expression::IntegerLiteral(1)),
                };
                self.assignment(target, &step)?
            }
            Expression::Comma { left, right } => {
                let mut out = self.effects_or_nothing(left)?;
                out.extend(self.effects_or_nothing(right)?);
                out
            }
            // `c ? a() : b();` and `x && f();` as statements are branches.
            Expression::Conditional { condition, when_true, when_false, .. } => {
                let condition = promoted(self.expression(condition)?);
                vec![Stmt::If {
                    condition,
                    then_body: self.effects_or_nothing(when_true)?,
                    else_body: self.effects_or_nothing(when_false)?,
                }]
            }
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), left, right } => {
                let condition = promoted(self.expression(left)?);
                let effects = self.effects_or_nothing(right)?;
                let condition = if *operator == BinaryOperator::LogicalAnd {
                    condition
                } else {
                    Expr::unary(UnaryOp::LogicalNot, condition, Type::Int)
                };
                vec![Stmt::If { condition, then_body: effects, else_body: Vec::new() }]
            }
            other => return Err(unsupported(format!("statement expression {}", expression_name(other)))),
        })
    }

    /// `field = value` for a bit-field: the containing unit is loaded, the
    /// value inserted (`rlwimi`) and the unit stored; a field filling its
    /// whole unit is a plain store.
    fn bit_field_store(&mut self, storage: &Expression, shift: u8, width: u8, value: &Expression) -> Compilation<Vec<Stmt>> {
        if self.unit.bit_field_declared_units {
            return Err(unsupported("bit-field store (declared-type units)"));
        }
        let (place, ty) = self.place(storage)?;
        let bits = 8 * mwcc_iro::width(ty) as u8;
        let value = promoted(self.expression(value)?);
        if is_float(value.ty) || shift + width > bits {
            return Err(unsupported("bit-field store of this value"));
        }
        if shift == 0 && width == bits {
            return Ok(vec![Stmt::Store { place, ty, value: assigned(value, ty) }]);
        }
        let old = self.expression(storage)?;
        let begin = 31 - (shift + width - 1);
        let end = 31 - shift;
        let inserted = Expr {
            kind: ExprKind::Idiom(mwcc_iro::Idiom::Insert { base: Box::new(old), value: Box::new(value), shift, begin, end }),
            ty: Type::Int,
        };
        Ok(vec![Stmt::Store { place, ty, value: inserted }])
    }

    /// `target = value` as a statement.
    fn assignment(&mut self, target: &Expression, value: &Expression) -> Compilation<Vec<Stmt>> {
        if let Expression::BitFieldRead { storage, shift, width, .. } = target {
            return self.bit_field_store(storage, *shift, *width, value);
        }
        if let Expression::Variable(name) = target {
            if let Some(&variable) = self.names.get(name).filter(|&&id| self.variables[id].frame.is_none()) {
                let ty = self.variables[variable].ty;
                return Ok(vec![Stmt::Assign { variable, value: assigned(self.expression(value)?, ty) }]);
            }
        }
        let (place, ty) = self.place(target)?;
        // A struct assignment copies the source object's bytes.
        if matches!(ty, Type::Struct { .. }) && std::env::var_os("MWCC_IRO_NO_STRUCT_COPY").is_none() {
            let source = self.address_of(value)?;
            let value = Expr { kind: ExprKind::Load { base: Box::new(source), index: None, offset: 0 }, ty };
            return Ok(vec![Stmt::Store { place, ty, value }]);
        }
        Ok(vec![Stmt::Store { place, ty, value: assigned(self.expression(value)?, ty) }])
    }

    /// A statement, preceded by the assignments hoisted out of its
    /// expressions.
    fn statement(&mut self, statement: &Statement) -> Compilation<Vec<Stmt>> {
        let outer = std::mem::take(&mut self.pending);
        let outer_post = std::mem::take(&mut self.post);
        let result = self.statement_inner(statement);
        let hoisted = std::mem::replace(&mut self.pending, outer);
        let after = std::mem::replace(&mut self.post, outer_post);
        let mut out = result?;
        if !hoisted.is_empty() {
            out.splice(0..0, hoisted);
        }
        if !after.is_empty() {
            match statement {
                // The step follows the statement that read the old value.
                Statement::Assign { .. } | Statement::Store { .. } | Statement::Expression(_) => out.extend(after),
                // A register variable's step after `return` is dead.
                Statement::Return(_) => {}
                // `if (*p++)`: the steps run before the branch, the tested
                // value copied first when they change it.
                Statement::If { .. } if matches!(out.last(), Some(Stmt::If { .. })) => {
                    let Some(Stmt::If { condition, .. }) = out.last_mut() else { unreachable!() };
                    let value = std::mem::replace(condition, Expr::int(0));
                    let mut before = Vec::new();
                    let value = self.frozen(value, &after, &mut before);
                    let Some(Stmt::If { condition, .. }) = out.last_mut() else { unreachable!() };
                    *condition = value;
                    before.extend(after);
                    let at = out.len() - 1;
                    out.splice(at..at, before);
                }
                _ => return Err(unsupported("post-increment in a control condition")),
            }
        }
        Ok(out)
    }

    /// A register for an intermediate value.
    fn temporary(&mut self, ty: Type) -> VarId {
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable { name: format!("@a{id}"), ty, kind: VariableKind::Temporary, frame: None, initialized: false, raw: false, volatile: false });
        id
    }

    /// A struct argument: copied into a caller frame object (before the
    /// call), whose address is passed.
    fn struct_argument(&mut self, argument: &Expression, size: u32, align: u8) -> Compilation<Expr> {
        if std::env::var_os("MWCC_IRO_NO_STRUCT_ARGUMENTS").is_some() {
            return Err(unsupported("struct argument"));
        }
        let ty = Type::Struct { size, align };
        let source = self.address_of(argument)?;
        if !matches!(source.ty, Type::StructPointer { element_size } if element_size == size) {
            return Err(unsupported("struct argument expression"));
        }
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable {
            name: format!("@s{id}"),
            ty,
            kind: VariableKind::Local,
            frame: Some((size, u32::from(align).max(1))),
            initialized: false,
            raw: false,
        volatile: false,
        });
        let pointer_type = Type::StructPointer { element_size: size };
        let copy = Expr { kind: ExprKind::LocalAddress(id), ty: pointer_type };
        let value = Expr { kind: ExprKind::Load { base: Box::new(source), index: None, offset: 0 }, ty };
        let store = Stmt::Store { place: Place::Memory { base: Box::new(copy.clone()), index: None, offset: 0 }, ty, value };
        self.pending.push(store);
        if !self.unit.unoptimized {
            return Ok(copy);
        }
        // -O0: the copy's address is a register variable, taken after it.
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable { name: format!("@p{id}"), ty: pointer_type, kind: VariableKind::Local, frame: None, initialized: false, raw: false, volatile: false });
        self.pending.push(Stmt::Assign { variable: id, value: copy });
        Ok(Expr { kind: ExprKind::Var(id), ty: pointer_type })
    }

    /// Whether a syntax expression denotes a whole struct object (its value
    /// is evaluated as its address).
    fn struct_valued(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(name) => match self.names.get(name) {
                Some(&id) => matches!(self.variables[id].frame, Some(_))
                    && !self.is_array(id)
                    && matches!(self.variables[id].ty, Type::Struct { .. }),
                None => self
                    .unit
                    .globals
                    .get(name)
                    .is_some_and(|global| !global.is_array && matches!(global.ty, Type::Struct { .. })),
            },
            Expression::Member { member_type: Type::Struct { .. }, .. } => true,
            _ => false,
        }
    }

    /// `target = value` used as a value: the assignment is hoisted and the
    /// assigned value is read from a register.
    fn assignment_value(&mut self, target: &Expression, value: &Expression) -> Compilation<Expr> {
        if self.guarded > 0 || std::env::var_os("MWCC_IRO_NO_ASSIGN_VALUE").is_some() {
            return Err(unsupported("expression Assign"));
        }
        if let Expression::Variable(name) = target {
            if let Some(&variable) = self.names.get(name).filter(|&&id| self.variables[id].frame.is_none()) {
                let ty = self.variables[variable].ty;
                let value = assigned(self.expression(value)?, ty);
                self.pending.push(Stmt::Assign { variable, value });
                return Ok(Expr { kind: ExprKind::Var(variable), ty });
            }
        }
        let (place, ty) = self.place(target)?;
        let value = self.expression(value)?;
        // A narrow integer target stores the raw value; reading the
        // assignment's value converts it.
        if is_narrow(ty) && !is_float(value.ty) && is_value_type(value.ty) && !is_narrow(value.ty) {
            let raw = value.ty;
            let temporary = self.temporary(raw);
            self.pending.push(Stmt::Assign { variable: temporary, value });
            let read = Expr { kind: ExprKind::Var(temporary), ty: raw };
            self.pending.push(Stmt::Store { place, ty, value: read.clone() });
            return Ok(Expr { kind: ExprKind::Convert(Box::new(read)), ty });
        }
        let value = assigned(value, ty);
        let temporary = self.temporary(ty);
        self.pending.push(Stmt::Assign { variable: temporary, value });
        self.pending.push(Stmt::Store { place, ty, value: Expr { kind: ExprKind::Var(temporary), ty } });
        Ok(Expr { kind: ExprKind::Var(temporary), ty })
    }

    /// An expression built where hoisting is not allowed.
    fn guarded_expression(&mut self, expression: &Expression) -> Compilation<Expr> {
        self.guarded += 1;
        let result = self.expression(expression);
        self.guarded -= 1;
        result
    }

    /// `switch`: arms in source order (the default last), empty labels
    /// sharing the next arm's body.
    fn switch(&mut self, scrutinee: &Expression, arms: &[ast::SwitchArm], default: Option<&ast::ArmBody>) -> Compilation<Stmt> {
        let mut value = promoted(self.expression(scrutinee)?);
        // (MWCC dispatches an unsigned word like an int: `cmpwi`.)
        if value.ty == Type::UnsignedInt {
            value.ty = Type::Int;
        }
        if !matches!(value.ty, Type::Int) {
            return Err(unsupported(format!("switch on {:?}", value.ty)));
        }
        let body = |builder: &mut Self, body: &ast::ArmBody, falls_through: bool| -> Compilation<Vec<Stmt>> {
            Ok(match body {
                ast::ArmBody::Return(value) => vec![Stmt::Return(Some(builder.returned(value)?))],
                ast::ArmBody::Statements(statements) => {
                    let mut out = builder.statements(statements)?;
                    let ends = matches!(out.last(), Some(Stmt::Return(_) | Stmt::Break | Stmt::Continue));
                    if !falls_through && !ends {
                        out.push(Stmt::Break);
                    }
                    out
                }
            })
        };
        let mut cases = Vec::new();
        let mut bodies: Vec<Vec<Stmt>> = Vec::new();
        let mut pending_labels: Vec<i64> = Vec::new();
        for arm in arms {
            if cases.iter().any(|&(value, _)| value == arm.value) || pending_labels.contains(&arm.value) {
                return Err(unsupported("duplicate case value"));
            }
            let empty = matches!(&arm.body, ast::ArmBody::Statements(statements) if statements.is_empty());
            if empty && arm.falls_through {
                pending_labels.push(arm.value);
                continue;
            }
            let index = bodies.len();
            for label in pending_labels.drain(..) {
                cases.push((label, index));
            }
            cases.push((arm.value, index));
            bodies.push(body(self, &arm.body, arm.falls_through)?);
        }
        if !pending_labels.is_empty() {
            // Trailing empty labels fall out of the switch.
            let index = bodies.len();
            for label in pending_labels.drain(..) {
                cases.push((label, index));
            }
            bodies.push(vec![Stmt::Break]);
        }
        let default = match default {
            Some(default) => {
                // The default is laid out last: a final case arm that falls
                // through leaves the switch instead of entering it.
                if let Some(last) = bodies.last_mut() {
                    if !matches!(last.last(), Some(Stmt::Return(_) | Stmt::Break | Stmt::Continue)) {
                        last.push(Stmt::Break);
                    }
                }
                bodies.push(body(self, default, true)?);
                Some(bodies.len() - 1)
            }
            None => None,
        };
        Ok(Stmt::Switch { value, cases, arms: bodies, default })
    }

    fn statement_inner(&mut self, statement: &Statement) -> Compilation<Vec<Stmt>> {
        Ok(vec![match statement {
            Statement::Switch { scrutinee, arms, default } => self.switch(scrutinee, arms, default.as_ref())?,
            // A frame variable, or a file-scope object (a function static
            // the parser saw as a local): a store.
            Statement::Assign { name, value }
                if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_some())
                    || (!self.names.contains_key(name) && self.unit.globals.contains_key(name)) =>
            {
                return self.assignment(&Expression::Variable(name.clone()), value);
            }
            Statement::Assign { name, value } => {
                let Some(&variable) = self.names.get(name) else {
                    return Err(unsupported(format!("assignment to non-local '{name}'")));
                };
                let ty = self.variables[variable].ty;
                Stmt::Assign { variable, value: assigned(self.expression(value)?, ty) }
            }
            Statement::Expression(expression) => return self.effects(expression),
            Statement::Loop { kind, initializer, condition, step, body } => {
                let mut out = match initializer {
                    Some(initializer) => self.effects(initializer)?,
                    None => Vec::new(),
                };
                let (pending_mark, post_mark) = (self.pending.len(), self.post.len());
                let guarded = condition.as_ref().map(|c| self.guarded_expression(c)).transpose();
                if guarded.is_err() {
                    // (Undo whatever the failed attempt queued.)
                    self.pending.truncate(pending_mark);
                    self.post.truncate(post_mark);
                }
                let (condition, effects) = match guarded {
                    Ok(condition) => (condition.map(promoted), Vec::new()),
                    // A condition with effects (`while ((c = *p++))`): they run
                    // before each test (in the loop's test block).
                    Err(_) if std::env::var_os("MWCC_IRO_NO_EFFECT_CONDITIONS").is_none() => {
                        let condition = condition.as_ref().expect("a failed condition exists");
                        let (pending, post) = (std::mem::take(&mut self.pending), std::mem::take(&mut self.post));
                        let value = promoted(self.expression(condition)?);
                        let mut effects = std::mem::replace(&mut self.pending, pending);
                        let after = std::mem::replace(&mut self.post, post);
                        let value = self.frozen(value, &after, &mut effects);
                        effects.extend(after);
                        (Some(value), effects)
                    }
                    Err(error) => return Err(error),
                };
                let body = self.statements(body)?;
                let step = match step {
                    Some(step) => self.effects(step)?,
                    None => Vec::new(),
                };
                out.push(Stmt::Loop { test_first: *kind != ast::LoopKind::DoWhile, condition, body, step, effects });
                return Ok(out);
            }
            Statement::Break => Stmt::Break,
            Statement::Goto(name) => Stmt::Goto(name.clone()),
            Statement::Label(name) => Stmt::Label(name.clone()),
            Statement::Continue => Stmt::Continue,
            Statement::Store { target, value } if matches!(target, Expression::BitFieldRead { .. }) => {
                return self.assignment(target, value);
            }
            // (A struct store copies its source through `assignment`.)
            Statement::Store { target, value } => return self.assignment(target, value),
            Statement::If { condition, then_body, else_body } => Stmt::If {
                condition: promoted(self.expression(condition)?),
                then_body: self.statements(then_body)?,
                else_body: self.statements(else_body)?,
            },
            Statement::Return(value) => {
                Stmt::Return(value.as_ref().map(|value| self.returned(value)).transpose()?)
            }
            other => return Err(unsupported(format!("statement {}", statement_name(other)))),
        }])
    }

    /// A store target and the type it holds.
    fn place(&mut self, target: &Expression) -> Compilation<(Place, Type)> {
        match target {
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                let (base, index, offset, ty) = displaced(base, *offset as i32, *member_type);
                Ok((Place::Memory { base, index, offset }, ty))
            }
            // An element of an embedded array of structs (`s->w[i]`): the
            // object itself.
            Expression::Index { base: array, .. }
                if matches!(array.as_ref(), Expression::Member { member_type: Type::Struct { .. }, .. }) =>
            {
                let Expression::Member { member_type: Type::Struct { size, align }, .. } = array.as_ref() else { unreachable!() };
                let (size, align) = (*size, *align);
                let address = self.address_of(target)?;
                Ok((Place::Memory { base: Box::new(address), index: None, offset: 0 }, Type::Struct { size, align }))
            }
            Expression::Index { base, index } => {
                let pointer = self.pointer_sum(base, index)?;
                match self.element(pointer)? {
                    Some((base, index, offset, ty)) => Ok((Place::Memory { base, index, offset }, ty)),
                    None => Err(unsupported("store to an index of a non-pointer")),
                }
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                // `*p` of a struct pointer: the object itself (a block copy's
                // destination; its alignment is unknown, which -O0 needs).
                if let Type::StructPointer { element_size } = pointer.ty {
                    if element_size > 0 && !self.unit.unoptimized {
                        let ty = Type::Struct { size: element_size, align: 4 };
                        return Ok((Place::Memory { base: Box::new(pointer), index: None, offset: 0 }, ty));
                    }
                }
                match self.element(pointer)? {
                    Some((base, index, offset, ty)) => Ok((Place::Memory { base, index, offset }, ty)),
                    None => Err(unsupported("store through a non-scalar pointer")),
                }
            }
            Expression::Variable(name) if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_some()) => {
                let id = self.names[name];
                let ty = self.variables[id].ty;
                Ok((Place::Memory { base: Box::new(self.local_address(id)), index: None, offset: 0 }, ty))
            }
            Expression::Variable(name) if self.unit.globals.contains_key(name) && !self.names.contains_key(name) => {
                let global = self.unit.globals[name];
                if global.is_array {
                    return Err(unsupported("store to an array global"));
                }
                Ok((Place::Global(name.clone()), global.ty))
            }
            _ => Err(unsupported("store target")),
        }
    }

    /// `base + index` as pointer arithmetic (for `base[index]`).
    fn pointer_sum(&mut self, base: &Expression, index: &Expression) -> Compilation<Expr> {
        let base = self.expression(base)?;
        let index = self.expression(index)?;
        if !matches!(base.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
            return Err(unsupported("index of a non-pointer"));
        }
        self.pointer_arithmetic(BinaryOp::Add, base, index)
    }

    /// Split a pointer value into a scalar element access:
    /// `(base, scaled index, offset, element type)`.
    #[allow(clippy::type_complexity)]
    fn element(&mut self, pointer: Expr) -> Compilation<Option<(Box<Expr>, Option<Box<Expr>>, i32, Type)>> {
        let Type::Pointer(pointee) = pointer.ty else { return Ok(None) };
        let Some(loaded) = pointee_type(pointee) else { return Err(unsupported("pointee type")) };
        if let ExprKind::Binary(BinaryOp::Add, left, right) = &pointer.kind {
            // `p + i` or the commuted `i + p`.
            let (base, index) = if matches!(right.ty, Type::Pointer(_)) { (right, left) } else { (left, right) };
            if matches!(base.ty, Type::Pointer(_)) && element_size(index.ty).is_none() {
                if let Some(offset) = index.as_int() {
                    if let Ok(offset) = i16::try_from(offset) {
                        return Ok(Some((base.clone(), None, i32::from(offset), loaded)));
                    }
                } else {
                    return Ok(Some((base.clone(), Some(index.clone()), 0, loaded)));
                }
            }
        }
        Ok(Some(displaced(pointer, 0, loaded)))
    }

    /// Whether a frame variable is an array (its name is its address).
    fn is_array(&self, id: VarId) -> bool {
        self.arrays.contains(&id)
    }

    /// The address of a frame variable, typed as a pointer to its element.
    fn local_address(&self, id: VarId) -> Expr {
        let ty = pointer_to(self.variables[id].ty).unwrap_or(Type::StructPointer { element_size: 0 });
        Expr { kind: ExprKind::LocalAddress(id), ty }
    }

    /// A returned value: floating results convert to the return type.
    fn returned(&mut self, value: &Expression) -> Compilation<Expr> {
        self.direct_return = matches!(value, Expression::Call { .. });
        let value = self.expression(value);
        self.direct_return = false;
        let value = value?;
        Ok(if is_float(self.return_type) || is_float(value.ty) { converted(value, self.return_type) } else { value })
    }

    /// `pointer ± integer` with the integer scaled to bytes.
    fn pointer_arithmetic(&mut self, op: BinaryOp, left: Expr, right: Expr) -> Compilation<Expr> {
        let left_size = element_size(left.ty);
        let right_size = element_size(right.ty);
        match (left_size, right_size) {
            // `p - q`: the byte difference divided (signed) by the element size.
            (Some(size), Some(other)) if op == BinaryOp::Subtract && size == other && size > 0 => {
                let as_int = |e: Expr| Expr { ty: Type::Int, ..e };
                let bytes = Expr::binary(BinaryOp::Subtract, as_int(left), as_int(right), Type::Int);
                Ok(if size == 1 { bytes } else { Expr::binary(BinaryOp::Divide, bytes, Expr::int(i64::from(size)), Type::Int) })
            }
            (Some(_), Some(_)) => Err(unsupported("pointer difference")),
            (Some(0), None) | (None, Some(0)) => Err(unsupported("arithmetic on an unsized pointee")),
            // `p + 0` (`&a[0]`) is the pointer itself.
            (Some(_), None) if right.as_int() == Some(0) && std::env::var_os("MWCC_IRO_NO_ZERO_OFFSET_FOLD").is_none() => Ok(left),
            (Some(size), None) => {
                let ty = left.ty;
                Ok(Expr::binary(op, left, scale(promoted(right), size), ty))
            }
            (None, Some(size)) if op == BinaryOp::Add => {
                let ty = right.ty;
                Ok(Expr::binary(op, scale(promoted(left), size), right, ty))
            }
            (None, Some(_)) => Err(unsupported("integer minus pointer")),
            (None, None) => unreachable!("pointer arithmetic needs a pointer"),
        }
    }

    fn expression(&mut self, expression: &Expression) -> Compilation<Expr> {
        Ok(match expression {
            Expression::IntegerLiteral(value) => Expr::int(*value),
            // A bare floating literal is `float`; a `double` one is cast.
            Expression::FloatLiteral(value) => Expr { kind: ExprKind::Float(*value), ty: Type::Float },
            Expression::Variable(name) if self.folded.contains_key(name) => self.folded[name].clone(),
            Expression::Variable(name) => {
                if let Some(&id) = self.names.get(name) {
                    let variable = &self.variables[id];
                    if variable.frame.is_some() {
                        let address = self.local_address(id);
                        // An array or struct names its address; a scalar loads.
                        if !matches!(address.ty, Type::StructPointer { .. }) && is_value_type(variable.ty) && !self.is_array(id) {
                            return Ok(Expr {
                                kind: ExprKind::Load { base: Box::new(address), index: None, offset: 0 },
                                ty: variable.ty,
                            });
                        }
                        return Ok(address);
                    }
                    return Ok(Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty });
                }
                let Some(global) = self.unit.globals.get(name) else {
                    return Err(unsupported(format!("unknown variable '{name}'")));
                };
                // A folded `static const` scalar is its value.
                if let Some(bits) = global.folded {
                    return Ok(match global.ty {
                        Type::Double => Expr { kind: ExprKind::Float(f64::from_bits(bits as u64)), ty: Type::Double },
                        Type::Float => Expr { kind: ExprKind::Float(f64::from(f32::from_bits(bits as u32))), ty: Type::Float },
                        ty if is_narrow(ty) => Expr::int(bits),
                        ty if is_value_type(ty) => Expr::typed_int(bits, ty),
                        _ => return Err(unsupported("a folded constant of this type")),
                    });
                }
                // A fixed-address array is its constant address.
                if let Some(address) = global.fixed_address {
                    let ty = pointer_to(global.ty).ok_or_else(|| unsupported("fixed-address array of this element type"))?;
                    return Ok(Expr { kind: ExprKind::Int(address), ty });
                }
                // An array (or aggregate) global denotes its address.
                if global.is_array || matches!(global.ty, Type::Struct { .. }) {
                    let ty = pointer_to(global.ty).ok_or_else(|| unsupported("array global of this element type"))?;
                    return Ok(Expr { kind: ExprKind::GlobalAddress(name.clone()), ty });
                }
                Expr { kind: ExprKind::Global(name.clone()), ty: global.ty }
            }
            Expression::Binary { operator, left, right } => {
                let op = binary_op(*operator);
                let left = self.expression(left)?;
                let right = if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    let mark = (self.pending.len(), self.post.len());
                    match self.guarded_expression(right) {
                        Ok(right) => right,
                        // (A right operand with effects: it runs only when
                        // the left one decides nothing; a temporary holds
                        // the truth value.)
                        Err(_) if self.guarded == 0 && std::env::var_os("MWCC_IRO_NO_BRANCHY_OPERANDS").is_none() => {
                            self.pending.truncate(mark.0);
                            self.post.truncate(mark.1);
                            let (right, mut body) = self.operand_with_effects(right)?;
                            let id = self.temporary(Type::Int);
                            let truth = match &right.kind {
                                ExprKind::Binary(op, ..) if op.is_comparison() || matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) => right,
                                ExprKind::Unary(UnaryOp::LogicalNot, _) => right,
                                _ => Expr::binary(BinaryOp::NotEqual, promoted(right), Expr::int(0), Type::Int),
                            };
                            body.push(Stmt::Assign { variable: id, value: truth });
                            let and = op == BinaryOp::LogicalAnd;
                            self.pending.push(Stmt::Assign { variable: id, value: Expr::int(i64::from(!and)) });
                            let condition = promoted(left);
                            let condition = if and { condition } else { Expr::unary(UnaryOp::LogicalNot, condition, Type::Int) };
                            self.pending.push(Stmt::If { condition, then_body: body, else_body: Vec::new() });
                            return Ok(Expr { kind: ExprKind::Var(id), ty: Type::Int });
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    self.expression(right)?
                };
                if matches!(op, BinaryOp::Add | BinaryOp::Subtract)
                    && (element_size(left.ty).is_some() || element_size(right.ty).is_some())
                {
                    return self.pointer_arithmetic(op, left, right);
                }
                // Floating operands meet at the wider floating type.
                if (is_float(left.ty) || is_float(right.ty)) && !matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    let common = if left.ty == Type::Double || right.ty == Type::Double { Type::Double } else { Type::Float };
                    let ty = if op.is_comparison() { Type::Int } else { common };
                    return Ok(Expr::binary(op, converted(left, common), converted(right, common), ty));
                }
                let ty = if op.is_comparison() || matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    Type::Int
                } else if matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight) {
                    promote(left.ty)
                } else {
                    arithmetic_type(left.ty, right.ty)
                };
                // The integer promotions are explicit conversions.
                let right = if op == BinaryOp::Subtract && self.unit.zero_wide_subtrahends {
                    wide_subtrahend(promoted(right), ty)
                } else {
                    promoted(right)
                };
                Expr::binary(op, promoted(left), right, ty)
            }
            Expression::Unary { operator, operand } => {
                let operand = promoted(self.expression(operand)?);
                match operator {
                    UnaryOperator::Negate => {
                        let ty = promote(operand.ty);
                        Expr::unary(UnaryOp::Negate, operand, ty)
                    }
                    UnaryOperator::BitNot => {
                        let ty = promote(operand.ty);
                        Expr::unary(UnaryOp::BitNot, operand, ty)
                    }
                    UnaryOperator::LogicalNot => Expr::unary(UnaryOp::LogicalNot, operand, Type::Int),
                }
            }
            // -O0: the value is a register variable assigned on each path.
            Expression::Conditional { condition, when_true, when_false, .. }
                if self.unit.unoptimized && self.guarded == 0 && std::env::var_os("MWCC_IRO_O0_SELECT_VALUE").is_none() =>
            {
                let condition = promoted(self.expression(condition)?);
                let outer = std::mem::take(&mut self.pending);
                let posts = self.post.len();
                let when_true = promoted(self.expression(when_true)?);
                let mut then_body = std::mem::take(&mut self.pending);
                let when_false = promoted(self.expression(when_false)?);
                let mut else_body = std::mem::take(&mut self.pending);
                self.pending = outer;
                if self.post.len() != posts {
                    return Err(unsupported("post-increment in a conditional operand"));
                }
                let ty = if is_float(when_true.ty) || is_float(when_false.ty) {
                    if when_true.ty == Type::Double || when_false.ty == Type::Double { Type::Double } else { Type::Float }
                } else {
                    arithmetic_type(when_true.ty, when_false.ty)
                };
                let id = self.variables.len() + self.temporaries.len();
                self.temporaries.push(Variable { name: format!("@c{id}"), ty, kind: VariableKind::Local, frame: None, initialized: false, raw: false, volatile: false });
                then_body.push(Stmt::Assign { variable: id, value: converted(when_true, ty) });
                else_body.push(Stmt::Assign { variable: id, value: converted(when_false, ty) });
                self.pending.push(Stmt::If { condition, then_body, else_body });
                Expr { kind: ExprKind::Var(id), ty }
            }
            Expression::Conditional { condition, when_true, when_false, .. } => {
                let condition = promoted(self.expression(condition)?);
                let mark = (self.pending.len(), self.post.len());
                let arms = self
                    .guarded_expression(when_true)
                    .and_then(|when_true| Ok((when_true, self.guarded_expression(when_false)?)));
                let (when_true, when_false) = match arms {
                    Ok(arms) => arms,
                    // (Operands with effects: branches assign a temporary.)
                    Err(_) if self.guarded == 0 && std::env::var_os("MWCC_IRO_NO_BRANCHY_OPERANDS").is_none() => {
                        self.pending.truncate(mark.0);
                        self.post.truncate(mark.1);
                        let (when_true, mut then_body) = self.operand_with_effects(when_true)?;
                        let (when_false, mut else_body) = self.operand_with_effects(when_false)?;
                        let ty = if is_float(when_true.ty) || is_float(when_false.ty) {
                            if when_true.ty == Type::Double || when_false.ty == Type::Double { Type::Double } else { Type::Float }
                        } else {
                            arithmetic_type(when_true.ty, when_false.ty)
                        };
                        let id = self.temporary(ty);
                        then_body.push(Stmt::Assign { variable: id, value: converted(when_true, ty) });
                        else_body.push(Stmt::Assign { variable: id, value: converted(when_false, ty) });
                        self.pending.push(Stmt::If { condition, then_body, else_body });
                        return Ok(Expr { kind: ExprKind::Var(id), ty });
                    }
                    Err(error) => return Err(error),
                };
                let when_true = promoted(when_true);
                let when_false = promoted(when_false);
                let ty = arithmetic_type(when_true.ty, when_false.ty);
                Expr {
                    kind: ExprKind::Select {
                        condition: Box::new(condition),
                        when_true: Box::new(when_true),
                        when_false: Box::new(when_false),
                    },
                    ty,
                }
            }
            Expression::Cast { target_type, operand } if is_value_type(*target_type) || is_wide(*target_type) => {
                let operand = self.expression(operand)?;
                Expr { kind: ExprKind::Convert(Box::new(operand)), ty: *target_type }
            }
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                if !is_value_type(*member_type) && !is_wide(*member_type) {
                    return Err(unsupported(format!("load of {member_type:?}")));
                }
                let (base, index, offset, ty) = displaced(base, *offset as i32, *member_type);
                Expr { kind: ExprKind::Load { base, index, offset }, ty }
            }
            Expression::AddressOf { operand } => self.address_of(operand)?,
            // Provenance wrappers: the value is the wrapped expression.
            Expression::IndexedUpdateValue { value } => self.expression(value)?,
            Expression::BitFieldRead { extracted, promoted_type, .. } => {
                let value = self.expression(extracted)?;
                converted(promoted(value), *promoted_type)
            }
            Expression::MemberAddress { base, offset, element, index_stride: None } => {
                let base = self.aggregate_address(base)?;
                let ty = pointee_type(*element).and_then(pointer_to).unwrap_or(Type::Pointer(*element));
                // (A member at offset 0 is the base itself, retyped.)
                if *offset == 0 && std::env::var_os("MWCC_IRO_MEMBER_ZERO_ADD").is_none() {
                    return Ok(Expr { ty, ..base });
                }
                Expr::binary(BinaryOp::Add, base, Expr::int(i64::from(*offset)), ty)
            }
            // A row of a multi-dimensional local array is its address.
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::Variable(name) if self.names.get(name).is_some_and(|id| self.rows.contains_key(id))) =>
            {
                let Expression::Variable(name) = base.as_ref() else { unreachable!() };
                let id = self.names[name];
                let row = self.rows[&id];
                let address = self.local_address(id);
                let index = self.expression(index)?;
                let ty = address.ty;
                Expr::binary(BinaryOp::Add, address, scale(promoted(index), row), ty)
            }
            Expression::Index { base, index } => {
                let pointer = self.pointer_sum(base, index)?;
                let Some((base, index, offset, ty)) = self.element(pointer)? else {
                    return Err(unsupported("index of a non-pointer"));
                };
                Expr { kind: ExprKind::Load { base, index, offset }, ty }
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                let Some((base, index, offset, ty)) = self.element(pointer)? else {
                    return Err(unsupported("dereference of a non-scalar pointer"));
                };
                Expr { kind: ExprKind::Load { base, index, offset }, ty }
            }
            Expression::Call { name, arguments } => self.call(name, arguments, false)?,
            Expression::CallThrough { target, arguments, return_type } => {
                self.indirect_call(target, arguments, false, *return_type)?
            }
            Expression::StringLiteral(bytes) => {
                if self.unit.strings_packed {
                    return Err(unsupported("packed string literals"));
                }
                let index = match self.strings.iter().position(|known| known == bytes) {
                    Some(index) => index,
                    None => {
                        self.strings.push(bytes.clone());
                        self.strings.len() - 1
                    }
                };
                Expr { kind: ExprKind::StringAddress(index), ty: Type::Pointer(mwcc_iro::Pointee::Char) }
            }
            Expression::Assign { target, value } => self.assignment_value(target, value)?,
            // `x++` as a value: the old value; the step follows the statement.
            // `(*p)++` / `s->n++` as a value: the old value is loaded into a
            // temporary and `old + 1` stored back before the statement.
            Expression::PostStep { target, operator, pointer_link: None }
                if pure_lvalue(target) && !matches!(target.as_ref(), Expression::Variable(_)) =>
            {
                if self.guarded > 0 || std::env::var_os("MWCC_IRO_NO_MEMORY_POST_VALUE").is_some() {
                    return Err(unsupported("expression PostStep"));
                }
                let old = self.expression(target)?;
                let ty = old.ty;
                let id = self.temporary(ty);
                self.pending.push(Stmt::Assign { variable: id, value: old });
                let (place, stored) = self.place(target)?;
                let current = Expr { kind: ExprKind::Var(id), ty };
                let op = if *operator == BinaryOperator::Subtract { BinaryOp::Subtract } else { BinaryOp::Add };
                let stepped = if matches!(ty, Type::Pointer(_) | Type::StructPointer { .. }) {
                    self.pointer_arithmetic(op, current.clone(), Expr::int(1))?
                } else {
                    Expr::binary(op, promoted(current.clone()), Expr::int(1), promote(ty))
                };
                self.pending.push(Stmt::Store { place, ty: stored, value: assigned(stepped, stored) });
                current
            }
            Expression::PostStep { target, operator, pointer_link: None }
                if matches!(target.as_ref(), Expression::Variable(name)
                    if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_none())) =>
            {
                if self.guarded > 0 || std::env::var_os("MWCC_IRO_NO_POST_VALUE").is_some() {
                    return Err(unsupported("expression PostStep"));
                }
                let Expression::Variable(name) = target.as_ref() else { unreachable!() };
                let id = self.names[name];
                let step = Expression::Binary {
                    operator: *operator,
                    left: target.clone(),
                    right: Box::new(Expression::IntegerLiteral(1)),
                };
                let statements = self.assignment(target, &step)?;
                self.post.extend(statements);
                Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty }
            }
            other => return Err(unsupported(format!("expression {}", expression_name(other)))),
        })
    }

    /// The address a member access is based on: a struct pointer value, or
    /// a struct object named directly (`s.m`).
    fn aggregate_address(&mut self, base: &Expression) -> Compilation<Expr> {
        match base {
            Expression::AddressOf { operand } => self.address_of(operand),
            // An element of an embedded array of structs (`s.homes[i].m`).
            Expression::Index { base: array, .. }
                if matches!(array.as_ref(), Expression::Member { member_type: Type::Struct { .. }, .. }) =>
            {
                self.address_of(base)
            }
            // An indexed struct element (`p[i].m`): the element's address;
            // an indexed pointer (`p[i]->m`) is loaded.
            Expression::Index { base: array, index } => {
                let sum = self.pointer_sum(array, index)?;
                if matches!(sum.ty, Type::StructPointer { .. }) {
                    Ok(sum)
                } else {
                    self.expression(base)
                }
            }
            other => self.expression(other),
        }
    }

    /// The struct address a member access is based on; `a[i].m` (with the
    /// struct size as `stride`) addresses `a + i*stride`.
    fn member_base(&mut self, base: &Expression, stride: Option<u32>) -> Compilation<Expr> {
        let Some(stride) = stride else { return self.aggregate_address(base) };
        let Expression::Index { base: array, index } = base else {
            return Err(unsupported("member of an indexed element"));
        };
        // An embedded array of structs (`o->pts[i].y`): the member's address,
        // then the scaled index.
        if let Expression::Member { base: outer, offset, member_type: Type::Struct { size, .. }, index_stride } = array.as_ref() {
            if *size == stride && std::env::var_os("MWCC_IRO_NO_MEMBER_STRUCT_ARRAYS").is_none() {
                // (The member offset last, so it joins the displacement.)
                let outer = self.member_base(outer, *index_stride)?;
                let ty = Type::StructPointer { element_size: stride };
                let index = self.expression(index)?;
                let element = Expr::binary(BinaryOp::Add, Expr { ty, ..outer }, scale(promoted(index), stride), ty);
                return Ok(Expr::binary(BinaryOp::Add, element, Expr::int(i64::from(*offset)), ty));
            }
        }
        let pointer = self.expression(array)?;
        // `a[i]->m` also records a stride: there the element is a pointer
        // to load, and only an array of the structs themselves is indexed.
        if !matches!(pointer.ty, Type::StructPointer { element_size } if element_size == stride) {
            return self.aggregate_address(base);
        }
        let index = self.expression(index)?;
        let ty = Type::StructPointer { element_size: stride };
        let pointer = Expr { ty, ..pointer };
        Ok(Expr::binary(BinaryOp::Add, pointer, scale(promoted(index), stride), ty))
    }

    /// `&operand`.
    fn address_of(&mut self, operand: &Expression) -> Compilation<Expr> {
        match operand {
            Expression::Variable(name) if !self.names.contains_key(name) => {
                let Some(global) = self.unit.globals.get(name) else {
                    return Err(unsupported(format!("unknown variable '{name}'")));
                };
                let ty = pointer_to(global.ty).ok_or_else(|| unsupported("address of this global type"))?;
                // (A folded `static const` has no object to address.)
                if global.folded.is_some() {
                    return Err(unsupported("address of a folded static constant"));
                }
                if let Some(address) = global.fixed_address {
                    return Ok(Expr { kind: ExprKind::Int(address), ty });
                }
                Ok(Expr { kind: ExprKind::GlobalAddress(name.clone()), ty })
            }
            Expression::Variable(name) => match self.names.get(name) {
                Some(&id) if self.variables[id].frame.is_some() => Ok(self.local_address(id)),
                _ => Err(unsupported("address of a register variable")),
            },
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                let ty = pointer_to(*member_type).ok_or_else(|| unsupported("address of this member type"))?;
                Ok(Expr::binary(BinaryOp::Add, base, Expr::int(i64::from(*offset)), ty))
            }
            // `&o->pts[i]`: an element of an embedded array of structs.
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::Member { member_type: Type::Struct { .. }, .. }) =>
            {
                let Expression::Member { member_type: Type::Struct { size, .. }, .. } = base.as_ref() else { unreachable!() };
                self.member_base(operand, Some(*size))
            }
            Expression::Index { base, index } => self.pointer_sum(base, index),
            Expression::Dereference { pointer } => self.expression(pointer),
            other => Err(unsupported(format!("address of {}", expression_name(other)))),
        }
    }

    /// A call; `discarded` when its result is unused (a `void` callee is fine).
    /// A call through a function pointer: `mtctr` + `bctrl`. Its value is
    /// modeled when the pointer's declared return type is known.
    fn indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
        discarded: bool,
        return_type: Option<Type>,
    ) -> Compilation<Expr> {
        let ty = match return_type {
            _ if discarded => Type::Void,
            Some(ty) if is_value_type(ty) => ty,
            _ => return Err(unsupported("value of an indirect call")),
        };
        if arguments.len() > ARGUMENT_REGISTERS {
            return Err(unsupported("stack-passed arguments"));
        }
        let target = self.guarded_expression(target)?;
        if !is_general_word(target.ty) {
            return Err(unsupported("call target of this type"));
        }
        let mut values = vec![target];
        for argument in arguments {
            let value = promoted(self.guarded_expression(argument)?);
            if is_float(value.ty) || !is_value_type(value.ty) {
                return Err(unsupported("indirect call argument of this type"));
            }
            values.push(value);
        }
        Ok(Expr { kind: ExprKind::Call { name: mwcc_iro::INDIRECT_CALL.to_owned(), arguments: values }, ty })
    }

    /// A call MWCC expands inline: the arguments are assigned to the
    /// callee's parameters, its body (renumbered into this function's
    /// temporaries) runs before the calling statement, and the call's value
    /// is the callee's final return value.
    fn inline_call(&mut self, callee: &ast::Function, arguments: &[Expression]) -> Compilation<Expr> {
        thread_local! {
            static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        // An expansion that is the returned value leaves its result in a
        // temporary (not a -O0 register variable).
        let direct_return = std::mem::take(&mut self.direct_return);
        if self.guarded > 0 {
            return Err(unsupported("inline expansion in a conditional operand"));
        }
        if callee.parameters.len() != arguments.len() {
            return Err(unsupported("inline expansion with mismatched arguments"));
        }
        if DEPTH.with(|depth| depth.get()) >= 4 {
            return Err(unsupported("nested inline expansion"));
        }
        DEPTH.with(|depth| depth.set(depth.get() + 1));
        let built = build_unoptimized(callee, self.unit);
        DEPTH.with(|depth| depth.set(depth.get() - 1));
        let built = built?;
        let mut inlined = built.function;
        // The expansion's string literals join the caller's.
        if !inlined.strings.is_empty() {
            let indices: Vec<usize> = inlined
                .strings
                .iter()
                .map(|bytes| match self.strings.iter().position(|known| known == bytes) {
                    Some(index) => index,
                    None => {
                        self.strings.push(bytes.clone());
                        self.strings.len() - 1
                    }
                })
                .collect();
            passes::map_strings(&mut inlined.body, &|index| indices[index]);
        }
        if inlined.variables.iter().any(|variable| variable.frame.is_some())
            && (!inlined.images.is_empty() || std::env::var_os("MWCC_IRO_NO_INLINE_FRAME_OBJECTS").is_some())
        {
            return Err(unsupported("inline expansion with frame objects"));
        }
        // Only a final return value: an early return would need a jump.
        let result = match inlined.body.last() {
            Some(Stmt::SetReturn(_)) => inlined.body.pop().map(|statement| match statement {
                Stmt::SetReturn(value) => value,
                _ => unreachable!("matched above"),
            }),
            _ => None,
        };
        // Early returns: the body becomes `do { ... } while (0)`, each
        // `return v` an assignment of the result and a `break` (not from
        // inside a loop or switch, where `break` means something else).
        let early = built.returns_through_variable || returns_anywhere(&inlined.body);
        if early && (std::env::var_os("MWCC_IRO_NO_INLINE_EARLY_RETURN").is_some() || returns_in_breakable(&inlined.body)) {
            return Err(unsupported("inline expansion with an early return"));
        }
        // At -O0 the expansion's variables are register variables.
        let kind = if self.unit.unoptimized { VariableKind::Local } else { VariableKind::Temporary };
        let base = self.variables.len() + self.temporaries.len();
        for variable in &inlined.variables {
            // (A frame object stays one: a local of the caller's frame.)
            self.temporaries.push(Variable {
                name: format!("{}${}", callee.name, variable.name),
                ty: variable.ty,
                kind: if variable.frame.is_some() { VariableKind::Local } else { kind },
                frame: variable.frame,
                initialized: false,
            raw: false,
            volatile: false,
            });
        }
        // Arguments are evaluated in order into the parameters; a constant
        // argument of a parameter the body never assigns is propagated.
        // The return value is renumbered and substituted with the body.
        let returns_value = result.is_some();
        if let Some(value) = result {
            inlined.body.push(Stmt::SetReturn(value));
        }
        let map = |id: VarId| id + base;
        passes::map_variables(&mut inlined.body, &map);
        for (index, argument) in arguments.iter().enumerate() {
            let value = self.expression(argument)?;
            let ty = inlined.variables[index].ty;
            let value = assigned(promoted(value), ty);
            let constant = value.as_int().is_some() && !is_float(ty);
            // (And a register variable of the same type.)
            let variable = matches!(value.kind, ExprKind::Var(id)
                if id < self.variables.len() && self.variables[id].frame.is_none() && self.variables[id].ty == ty)
                && std::env::var_os("MWCC_IRO_NO_INLINE_VARIABLES").is_none();
            // (And arithmetic on those, re-evaluated at each use.)
            fn pure(e: &Expr, variables: &[Variable]) -> bool {
                match &e.kind {
                    ExprKind::Int(_) => true,
                    ExprKind::Var(id) => *id < variables.len() && variables[*id].frame.is_none(),
                    ExprKind::Binary(op, a, b) => {
                        !matches!(op, BinaryOp::Divide | BinaryOp::Modulo | BinaryOp::LogicalAnd | BinaryOp::LogicalOr)
                            && pure(a, variables)
                            && pure(b, variables)
                    }
                    ExprKind::Unary(_, a) | ExprKind::Convert(a) => pure(a, variables),
                    _ => false,
                }
            }
            let expression = !is_float(ty)
                && !is_float(value.ty)
                && pure(&value, &self.variables)
                && std::env::var_os("MWCC_IRO_NO_INLINE_EXPRESSIONS").is_none();
            if (constant || variable || expression)
                && !body_assigns(&inlined.body, base + index)
                && std::env::var_os("MWCC_IRO_NO_INLINE_CONSTANTS").is_none()
            {
                // (Converted to the parameter's type where it differs.)
                let substituted = if expression && !constant && value.ty != ty {
                    Expr { kind: ExprKind::Convert(Box::new(value.clone())), ty }
                } else {
                    Expr { kind: value.kind.clone(), ty }
                };
                passes::substitute(&mut inlined.body, base + index, &substituted);
                continue;
            }
            self.pending.push(Stmt::Assign { variable: base + index, value });
        }
        let value = match returns_value.then(|| inlined.body.pop()).flatten() {
            Some(Stmt::SetReturn(value)) => Some(value),
            _ => None,
        };
        if early {
            let ty = callee.return_type;
            let id = self.variables.len() + self.temporaries.len();
            self.temporaries.push(Variable { name: format!("{}$result", callee.name), ty, kind, frame: None, initialized: false, raw: false, volatile: false });
            let result = (ty != Type::Void).then_some(id);
            let mut body = early_returns(std::mem::take(&mut inlined.body), result, ty);
            if let (Some(id), Some(value)) = (result, value) {
                body.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
            }
            self.pending.push(Stmt::Loop {
                test_first: false,
                condition: Some(Expr::int(0)),
                body,
                step: Vec::new(),
                effects: Vec::new(),
            });
            return Ok(match result {
                Some(id) => Expr { kind: ExprKind::Var(id), ty },
                None => Expr { kind: ExprKind::Int(0), ty: Type::Void },
            });
        }
        let body_was_empty = inlined.body.is_empty() && !std::env::var_os("MWCC_IRO_INLINE_VALUE_TEMPORARY").is_some();
        self.pending.extend(inlined.body);
        let inlined = mwcc_iro::Function { body: Vec::new(), ..inlined };
        let Some(value) = value else {
            return Ok(Expr { kind: ExprKind::Int(0), ty: Type::Void });
        };
        // (Optimized, an expansion that is only its value is that value.)
        let only_value = !self.unit.unoptimized && inlined.body.is_empty() && body_was_empty;
        if direct_return || only_value {
            // (Still converted to the callee's return type.)
            return Ok(converted(value, callee.return_type));
        }
        // The value is held in a temporary of the callee's return type.
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable {
            name: format!("{}$result", callee.name),
            ty: callee.return_type,
            kind,
            frame: None,
            initialized: false,
            raw: false,
        volatile: false,
        });
        self.pending.push(Stmt::Assign { variable: id, value: assigned(value, callee.return_type) });
        Ok(Expr { kind: ExprKind::Var(id), ty: callee.return_type })
    }

    /// A compiler intrinsic: `__rlwimi` inserts, `__cntlzw` counts,
    /// `__fabs` takes the absolute value.
    fn intrinsic(&mut self, name: &str, arguments: &[Expression]) -> Compilation<Expr> {
        match (name, arguments) {
            ("__rlwimi", [base, value, shift, begin, end]) => {
                let constant = |e: &Expression| match e {
                    Expression::IntegerLiteral(v) if (0..32).contains(v) => Some(*v as u8),
                    _ => None,
                };
                let (Some(shift), Some(begin), Some(end)) = (constant(shift), constant(begin), constant(end)) else {
                    return Err(unsupported("intrinsic '__rlwimi' with variable fields"));
                };
                let base = self.expression(base)?;
                let value = self.expression(value)?;
                Ok(Expr {
                    kind: ExprKind::Idiom(Idiom::Insert {
                        base: Box::new(base),
                        value: Box::new(value),
                        shift,
                        begin,
                        end,
                    }),
                    ty: Type::Int,
                })
            }
            ("__cntlzw", [value]) => {
                let value = self.expression(value)?;
                Ok(Expr { kind: ExprKind::Idiom(Idiom::Unary(IntrinsicOp::CountLeadingZeros, Box::new(value))), ty: Type::Int })
            }
            ("__fabs", [value]) => {
                let value = converted(self.expression(value)?, Type::Double);
                Ok(Expr { kind: ExprKind::Idiom(Idiom::Unary(IntrinsicOp::FloatAbsolute, Box::new(value))), ty: Type::Double })
            }
            _ => Err(unsupported(format!("intrinsic '{name}'"))),
        }
    }

    fn call(&mut self, name: &str, arguments: &[Expression], discarded: bool) -> Compilation<Expr> {
        let direct_return = std::mem::take(&mut self.direct_return);
        // A call through a pointer variable (local, parameter or global).
        let pointer_variable = self.names.contains_key(name)
            || self.unit.globals.get(name).is_some_and(|global| !global.is_function);
        if pointer_variable {
            let owner = if self.names.contains_key(name) { self.function_name } else { "" };
            let return_type = (self.unit.pointer_return_type)(owner, name);
            return self.indirect_call(&Expression::Variable(name.to_owned()), arguments, discarded, return_type);
        }
        {
            {
                let ty = self.unit.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_value_type(ty) && !is_wide(ty) && !(discarded && ty == Type::Void) {
                    return Err(unsupported("non-integer call result"));
                }
                if (self.unit.is_intrinsic)(name, arguments.len()) {
                    return self.intrinsic(name, arguments);
                }
                let variadic = self.unit.variadic_callees.contains(name);
                let prototyped = self.unit.prototyped.contains(name);
                if (self.unit.has_body)(name) {
                    return match self.unit.inline_bodies.get(name).copied() {
                        Some(callee) if std::env::var_os("MWCC_IRO_NO_INLINE").is_none() => {
                            self.direct_return = direct_return;
                            self.inline_call(callee, arguments)
                        }
                        _ => Err(unsupported("call to a function this unit defines (inlining not modeled)")),
                    };
                }
                if arguments.len() > ARGUMENT_REGISTERS {
                    return Err(unsupported("stack-passed arguments"));
                }
                // An expanded call among arguments that are otherwise literals
                // or register variables may run its body first.
                let simple = |builder: &Self, argument: &Expression| match argument {
                    Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
                    Expression::Variable(name) => {
                        builder.names.get(name).is_some_and(|&id| builder.variables[id].frame.is_none())
                    }
                    _ => false,
                };
                let parameter_types = self.unit.call_parameter_types.get(name).cloned();
                let mut values = Vec::with_capacity(arguments.len());
                for (index, argument) in arguments.iter().enumerate() {
                    // A struct passed by value: a caller copy, by address.
                    match parameter_types.as_ref().and_then(|types| types.get(index)).copied() {
                        Some(Type::Struct { size, align }) if !variadic || index < parameter_types.as_ref().map_or(0, Vec::len) => {
                            values.push(self.struct_argument(argument, size, align)?);
                            continue;
                        }
                        _ if self.struct_valued(argument) => return Err(unsupported("struct argument without a struct parameter")),
                        _ => {}
                    }
                    let expanded = matches!(argument, Expression::Call { name, .. } if self.unit.inline_bodies.contains_key(name));
                    let others_simple = arguments.iter().enumerate().all(|(other, value)| other == index || simple(self, value));
                    values.push(if expanded && others_simple && self.guarded == 0 {
                        self.expression(argument)?
                    } else {
                        self.guarded_expression(argument)?
                    });
                }
                let mut arguments = values;
                if variadic {
                    // Arguments past the fixed parameters take the default
                    // promotions (floating ones widen to double).
                    let fixed = self.unit.call_parameter_types.get(name).map_or(0, |types| types.len());
                    for argument in arguments.iter_mut().skip(fixed) {
                        let value = std::mem::replace(argument, Expr::int(0));
                        *argument = if is_float(value.ty) { converted(value, Type::Double) } else { promoted(value) };
                    }
                }
                if !prototyped {
                    // Default argument promotions (a floating one widens to
                    // double; MWCC sets no CR bit for these calls).
                    let arguments = arguments
                        .into_iter()
                        .map(|argument| if is_float(argument.ty) { converted(argument, Type::Double) } else { promoted(argument) })
                        .collect();
                    return Ok(Expr { kind: ExprKind::Call { name: name.to_owned(), arguments }, ty });
                }
                // The caller converts an argument to a narrow parameter's type.
                if let Some(types) = self.unit.call_parameter_types.get(name) {
                    for (argument, &parameter) in arguments.iter_mut().zip(types) {
                        if is_narrow(parameter)
                            && argument.ty != parameter
                            && !passes::fits_unconverted(argument, parameter)
                        {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = Expr { kind: ExprKind::Convert(Box::new(value)), ty: parameter };
                        } else if is_float(parameter) || is_float(argument.ty) {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = converted(value, parameter);
                        } else if is_wide(parameter) || is_wide(argument.ty) {
                            // (To or from `long long`: converted.)
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = converted(promoted(value), parameter);
                        } else if !is_narrow(parameter) {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = promoted(value);
                        }
                    }
                }
                if arguments.iter().any(|argument| !is_value_type(argument.ty) && !is_wide(argument.ty)) {
                    return Err(unsupported("argument type"));
                }
                let floats = arguments.iter().filter(|argument| is_float(argument.ty)).count();
                // (A wide argument takes an odd-aligned register pair.)
                let generals = arguments.iter().filter(|argument| !is_float(argument.ty)).fold(0usize, |used, argument| {
                    if is_wide(argument.ty) { used + used % 2 + 2 } else { used + 1 }
                });
                if floats > ARGUMENT_REGISTERS || generals > ARGUMENT_REGISTERS {
                    return Err(unsupported("stack-passed arguments"));
                }
                Ok(Expr { kind: ExprKind::Call { name: name.to_owned(), arguments }, ty })
            }
        }
    }
}

/// `index * size` for pointer arithmetic, folded when the index is constant.
fn scale(index: Expr, size: u32) -> Expr {
    if size == 1 {
        return index;
    }
    match index.as_int() {
        Some(value) => Expr::int(value * i64::from(size)),
        None => {
            let ty = promote(index.ty);
            Expr::binary(BinaryOp::Multiply, index, Expr::int(i64::from(size)), ty)
        }
    }
}

/// The usual arithmetic conversion of two integer operands.
/// MWCC subtracts a signed word converted to (signed) `long long` with a
/// zero high word: the conversion zero-extends there.
fn wide_subtrahend(right: Expr, ty: Type) -> Expr {
    // (Only a signed difference: an unsigned one sign-extends the word.)
    if ty != Type::LongLong || std::env::var_os("MWCC_IRO_WIDE_SUBTRAHEND_EXTENDED").is_some() {
        return right;
    }
    let word = match &right.kind {
        ExprKind::Convert(word) if right.ty == Type::LongLong && !is_wide(word.ty) => (**word).clone(),
        _ if !is_wide(right.ty) => right.clone(),
        _ => return right,
    };
    if is_unsigned(promote(word.ty)) || is_float(word.ty) || word.as_int().is_some() {
        return right;
    }
    let word = promoted(word);
    let unsigned = Expr { kind: ExprKind::Convert(Box::new(word)), ty: Type::UnsignedInt };
    Expr { kind: ExprKind::Convert(Box::new(unsigned)), ty }
}

fn arithmetic_type(left: Type, right: Type) -> Type {
    let (left, right) = (promote(left), promote(right));
    // (`long long` wins; unsigned when either is unsigned `long long`.)
    if is_wide(left) || is_wide(right) {
        return if left == Type::UnsignedLongLong || right == Type::UnsignedLongLong {
            Type::UnsignedLongLong
        } else {
            Type::LongLong
        };
    }
    if matches!(left, Type::Pointer(_) | Type::StructPointer { .. }) {
        return left;
    }
    if is_unsigned(left) || is_unsigned(right) {
        Type::UnsignedInt
    } else {
        left
    }
}

fn binary_op(operator: BinaryOperator) -> BinaryOp {
    match operator {
        BinaryOperator::Add => BinaryOp::Add,
        BinaryOperator::Subtract => BinaryOp::Subtract,
        BinaryOperator::Multiply => BinaryOp::Multiply,
        BinaryOperator::Divide => BinaryOp::Divide,
        BinaryOperator::Modulo => BinaryOp::Modulo,
        BinaryOperator::BitAnd => BinaryOp::BitAnd,
        BinaryOperator::BitOr => BinaryOp::BitOr,
        BinaryOperator::BitXor => BinaryOp::BitXor,
        BinaryOperator::ShiftLeft => BinaryOp::ShiftLeft,
        BinaryOperator::ShiftRight => BinaryOp::ShiftRight,
        BinaryOperator::Less => BinaryOp::Less,
        BinaryOperator::Greater => BinaryOp::Greater,
        BinaryOperator::LessEqual => BinaryOp::LessEqual,
        BinaryOperator::GreaterEqual => BinaryOp::GreaterEqual,
        BinaryOperator::Equal => BinaryOp::Equal,
        BinaryOperator::NotEqual => BinaryOp::NotEqual,
        BinaryOperator::LogicalAnd => BinaryOp::LogicalAnd,
        BinaryOperator::LogicalOr => BinaryOp::LogicalOr,
    }
}

fn statement_name(statement: &Statement) -> &'static str {
    match statement {
        Statement::Store { .. } => "store",
        Statement::Assign { .. } => "assign",
        Statement::Expression(_) => "expression",
        Statement::InlineAsm(_) => "inline asm",
        Statement::If { .. } => "if",
        Statement::Return(_) => "return",
        Statement::Switch { .. } => "switch",
        Statement::Break => "break",
        Statement::Continue => "continue",
        Statement::Goto(_) => "goto",
        Statement::Label(_) => "label",
        Statement::Loop { .. } => "loop",
    }
}

/// The variant name of an expression (for refusal diagnostics).
/// A literal initializer's value as `ty` (a scalar), when it is one.
fn literal_value(value: &Expression, ty: Type) -> Option<Expr> {
    fn number(value: &Expression) -> Option<f64> {
        match value {
            Expression::IntegerLiteral(v) => Some(*v as f64),
            Expression::FloatLiteral(v) => Some(*v),
            Expression::Cast { operand, .. } => number(operand),
            Expression::Unary { operator: ast::UnaryOperator::Negate, operand } => number(operand).map(|v| -v),
            _ => None,
        }
    }
    fn integer(value: &Expression) -> Option<i64> {
        match value {
            Expression::IntegerLiteral(v) => Some(*v),
            Expression::Cast { operand, .. } => integer(operand),
            Expression::Unary { operator: ast::UnaryOperator::Negate, operand } => integer(operand).map(|v| -v),
            _ => None,
        }
    }
    match ty {
        Type::Double => Some(Expr { kind: ExprKind::Float(number(value)?), ty }),
        Type::Float => Some(Expr { kind: ExprKind::Float(f64::from(number(value)? as f32)), ty }),
        ty if is_narrow(ty) => Some(Expr::int(integer(value)?)),
        ty if (is_value_type(ty) || is_wide(ty)) && !is_float(ty) => Some(Expr::typed_int(integer(value)?, ty)),
        _ => None,
    }
}

fn expression_name(expression: &Expression) -> String {
    let debug = format!("{expression:?}");
    let end = debug.find([' ', '(', '{']).unwrap_or(debug.len());
    let mut name = debug[..end].to_owned();
    match expression {
        Expression::Cast { target_type, .. } => name.push_str(&format!(" to {target_type:?}")),
        Expression::Member { index_stride: Some(_), .. } => name.push_str(" of an indexed element"),
        _ => {}
    }
    name
}

/// `e` after the integer promotions: a narrow value converts to `int`.
pub fn promoted(e: Expr) -> Expr {
    if is_narrow(e.ty) {
        let ty = promote(e.ty);
        Expr { kind: ExprKind::Convert(Box::new(e)), ty }
    } else {
        e
    }
}

/// A value assigned or stored as `ty`: a narrow value promotes to a word;
/// a floating value (or destination) converts.
fn assigned(value: Expr, ty: Type) -> Expr {
    if is_float(ty) || is_float(value.ty) {
        converted(value, ty)
    } else if is_narrow(ty) {
        value
    } else {
        promoted(value)
    }
}

/// `value` converted to `ty` (unchanged when it already has that type).
fn converted(value: Expr, ty: Type) -> Expr {
    if value.ty == ty {
        value
    } else {
        Expr { kind: ExprKind::Convert(Box::new(value)), ty }
    }
}

/// `pointer + offset` as a memory operand, a constant addend of the pointer
/// folded into the displacement.
fn displaced(pointer: Expr, offset: i32, ty: Type) -> (Box<Expr>, Option<Box<Expr>>, i32, Type) {
    // Look through a conversion between pointer types.
    if let ExprKind::Convert(operand) = &pointer.kind {
        if matches!(operand.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
            return displaced((**operand).clone(), offset, ty);
        }
    }
    if let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract), base, addend) = &pointer.kind {
        if let Some(value) = addend.as_int() {
            let value = if *op == BinaryOp::Subtract { -value } else { value };
            if let Ok(total) = i16::try_from(i64::from(offset) + value) {
                if matches!(base.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
                    return displaced((**base).clone(), i32::from(total), ty);
                }
            }
        }
    }
    // `p + i` at displacement 0 is an indexed access.
    if let (ExprKind::Binary(BinaryOp::Add, base, index), 0) = (&pointer.kind, offset) {
        if matches!(base.ty, Type::Pointer(_) | Type::StructPointer { .. })
            && element_size(index.ty).is_none()
            && index.as_int().is_none()
            && std::env::var_os("MWCC_IRO_NO_MEMBER_INDEXED").is_none()
        {
            return (base.clone(), Some(index.clone()), 0, ty);
        }
    }
    (Box::new(pointer), None, offset, ty)
}

/// Names whose address the function takes (`&name`).
fn addresses_taken(function: &ast::Function) -> std::collections::HashSet<String> {
    fn expression(e: &Expression, out: &mut std::collections::HashSet<String>) {
        if let Expression::AddressOf { operand } = e {
            if let Expression::Variable(name) = operand.as_ref() {
                out.insert(name.clone());
            }
        }
        match e {
            Expression::Binary { left, right, .. } => {
                expression(left, out);
                expression(right, out);
            }
            Expression::Unary { operand, .. }
            | Expression::Cast { operand, .. }
            | Expression::AddressOf { operand } => expression(operand, out),
            Expression::Dereference { pointer } => expression(pointer, out),
            Expression::Index { base, index } => {
                expression(base, out);
                expression(index, out);
            }
            Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => expression(base, out),
            Expression::Conditional { condition, when_true, when_false, .. } => {
                expression(condition, out);
                expression(when_true, out);
                expression(when_false, out);
            }
            Expression::Call { arguments, .. } => arguments.iter().for_each(|a| expression(a, out)),
            Expression::Assign { target, value } => {
                expression(target, out);
                expression(value, out);
            }
            Expression::Comma { left, right } => {
                expression(left, out);
                expression(right, out);
            }
            Expression::IndexedUpdateValue { value } => expression(value, out),
            Expression::PostStep { target, .. } => expression(target, out),
            _ => {}
        }
    }
    fn statements(body: &[Statement], out: &mut std::collections::HashSet<String>) {
        for statement in body {
            match statement {
                Statement::Assign { value, .. } => expression(value, out),
                Statement::Store { target, value } => {
                    expression(target, out);
                    expression(value, out);
                }
                Statement::Expression(e) => expression(e, out),
                Statement::If { condition, then_body, else_body } => {
                    expression(condition, out);
                    statements(then_body, out);
                    statements(else_body, out);
                }
                Statement::Return(Some(e)) => expression(e, out),
                Statement::Loop { initializer, condition, step, body, .. } => {
                    for e in [initializer, condition, step].into_iter().flatten() {
                        expression(e, out);
                    }
                    statements(body, out);
                }
                Statement::Switch { scrutinee, arms, default } => {
                    expression(scrutinee, out);
                    for body in arms.iter().map(|arm| &arm.body).chain(default.iter()) {
                        match body {
                            ast::ArmBody::Return(value) => expression(value, out),
                            ast::ArmBody::Statements(body) => statements(body, out),
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    statements(&function.statements, &mut out);
    for guard in &function.guards {
        expression(&guard.condition, &mut out);
        expression(&guard.value, &mut out);
    }
    if let Some(value) = &function.return_expression {
        expression(value, &mut out);
    }
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            expression(initializer, &mut out);
        }
    }
    out
}
