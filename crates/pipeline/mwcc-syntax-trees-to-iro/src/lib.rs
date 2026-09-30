//! Build the typed IRO representation of a function from its syntax tree,
//! then run the IRO passes (see [`passes`]).
//!
//! The builder resolves every expression's type, makes pointer arithmetic
//! explicit (indices are scaled to bytes), turns member, index and
//! dereference accesses into loads and stores through `base + index +
//! offset`, and turns guards and the final return into statements. It
//! rejects (with a diagnostic) anything the PCode pipeline does not model.

pub mod passes;

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_iro::{
    element_size, is_float, is_general_word, is_narrow, is_unsigned, is_value_type, pointee_type, pointer_to, promote,
    BinaryOp, Expr, ExprKind,
    Function, Place, Stmt, Type, UnaryOp, Unit, VarId, Variable, VariableKind,
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
    passes::run(&mut built.function);
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
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return Err(unsupported("inline assembly"));
    }
    if function.return_type != Type::Void && !is_value_type(function.return_type) {
        return Err(unsupported(format!("return type {:?}", function.return_type)));
    }
    let mut variables = Vec::new();
    for parameter in &function.parameters {
        if !is_value_type(parameter.parameter_type) {
            return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
        }
        variables.push(Variable {
            name: parameter.name.clone(),
            ty: parameter.parameter_type,
            kind: VariableKind::Parameter,
            frame: None,
        });
    }
    let floats = function.parameters.iter().filter(|p| is_float(p.parameter_type)).count();
    if function.parameters.len() - floats > ARGUMENT_REGISTERS || floats > ARGUMENT_REGISTERS {
        return Err(unsupported("stack-passed parameters"));
    }
    let taken = addresses_taken(function);
    for local in &function.locals {
        if local.is_static || local.is_volatile {
            return Err(unsupported("static or volatile locals"));
        }
        // Arrays, structs and scalars whose address is taken live in the frame.
        let element = match local.declared_type {
            Type::Struct { size, align } => Some((size, u32::from(align).max(1))),
            ty if is_value_type(ty) => Some((mwcc_iro::width(ty), mwcc_iro::width(ty))),
            _ => None,
        };
        let frame = match (local.array_length, element) {
            (Some(length), Some((size, align))) => Some((size * u32::from(length), align)),
            (None, Some(object)) if matches!(local.declared_type, Type::Struct { .. }) || taken.contains(&local.name) => {
                Some(object)
            }
            (None, Some(_)) => None,
            _ => return Err(unsupported(format!("local type {:?}", local.declared_type))),
        };
        if frame.is_some() && local.data_bytes.is_some() {
            return Err(unsupported("an initialized frame array"));
        }
        variables.push(Variable { name: local.name.clone(), ty: local.declared_type, kind: VariableKind::Local, frame });
    }
    if function.parameters.iter().any(|parameter| taken.contains(&parameter.name)) {
        return Err(unsupported("address of a parameter"));
    }
    let arrays = function
        .locals
        .iter()
        .filter(|local| local.array_length.is_some())
        .filter_map(|local| variables.iter().position(|variable| variable.name == local.name && variable.kind == VariableKind::Local))
        .collect();
    let mut builder = Builder {
        arrays,
        return_type: function.return_type,
        unit,
        names: variables.iter().enumerate().map(|(id, variable)| (variable.name.clone(), id)).collect(),
        variables: &variables,
        pending: Vec::new(),
        post: Vec::new(),
        strings: Vec::new(),
        guarded: 0,
        temporaries: Vec::new(),
    };
    let mut body = Vec::new();
    for local in &function.locals {
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
        let condition = promoted(builder.expression(&guard.condition)?);
        if !builder.post.is_empty() {
            return Err(unsupported("post-increment in a control condition"));
        }
        body.append(&mut builder.pending);
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
            name: function.name.clone(),
            return_type: function.return_type,
            parameter_count: function.parameters.len(),
            variables,
            body,
        },
        returns_through_variable,
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

struct Builder<'a> {
    /// Frame variables that are arrays.
    arrays: Vec<VarId>,
    return_type: Type,
    unit: &'a Unit<'a>,
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
}

impl Builder<'_> {
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

    fn effects_inner(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        Ok(match expression {
            Expression::Call { name, arguments } => vec![Stmt::Eval(self.call(name, arguments, true)?)],
            Expression::CallThrough { target, arguments } => vec![Stmt::Eval(self.indirect_call(target, arguments, true)?)],
            Expression::Cast { target_type: Type::Void, operand } => match operand.as_ref() {
                Expression::Call { name, arguments } => vec![Stmt::Eval(self.call(name, arguments, true)?)],
                Expression::Variable(_) | Expression::IntegerLiteral(_) => Vec::new(),
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
                let mut out = self.effects(left)?;
                out.extend(self.effects(right)?);
                out
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
                _ => return Err(unsupported("post-increment in a control condition")),
            }
        }
        Ok(out)
    }

    /// A register for an intermediate value.
    fn temporary(&mut self, ty: Type) -> VarId {
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable { name: format!("@a{id}"), ty, kind: VariableKind::Temporary, frame: None });
        id
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
        let value = promoted(self.expression(scrutinee)?);
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
            Statement::Assign { name, value } if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_some()) => {
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
                let condition = condition.as_ref().map(|c| self.guarded_expression(c)).transpose()?.map(promoted);
                let body = self.statements(body)?;
                let step = match step {
                    Some(step) => self.effects(step)?,
                    None => Vec::new(),
                };
                out.push(Stmt::Loop { test_first: *kind != ast::LoopKind::DoWhile, condition, body, step });
                return Ok(out);
            }
            Statement::Break => Stmt::Break,
            Statement::Continue => Stmt::Continue,
            Statement::Store { target, value } if matches!(target, Expression::BitFieldRead { .. }) => {
                return self.assignment(target, value);
            }
            Statement::Store { target, value } => {
                let (place, ty) = self.place(target)?;
                Stmt::Store { place, ty, value: assigned(self.expression(value)?, ty) }
            }
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
            Expression::Index { base, index } => {
                let pointer = self.pointer_sum(base, index)?;
                match self.element(pointer)? {
                    Some((base, index, offset, ty)) => Ok((Place::Memory { base, index, offset }, ty)),
                    None => Err(unsupported("store to an index of a non-pointer")),
                }
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
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
        let value = self.expression(value)?;
        Ok(if is_float(self.return_type) || is_float(value.ty) { converted(value, self.return_type) } else { value })
    }

    /// `pointer ± integer` with the integer scaled to bytes.
    fn pointer_arithmetic(&mut self, op: BinaryOp, left: Expr, right: Expr) -> Compilation<Expr> {
        let left_size = element_size(left.ty);
        let right_size = element_size(right.ty);
        match (left_size, right_size) {
            (Some(_), Some(_)) => Err(unsupported("pointer difference")),
            (Some(0), None) | (None, Some(0)) => Err(unsupported("arithmetic on an unsized pointee")),
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
                    self.guarded_expression(right)?
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
                Expr::binary(op, promoted(left), promoted(right), ty)
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
            Expression::Conditional { condition, when_true, when_false, .. } => {
                let condition = promoted(self.expression(condition)?);
                let when_true = promoted(self.guarded_expression(when_true)?);
                let when_false = promoted(self.guarded_expression(when_false)?);
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
            Expression::Cast { target_type, operand } if is_value_type(*target_type) => {
                let operand = self.expression(operand)?;
                Expr { kind: ExprKind::Convert(Box::new(operand)), ty: *target_type }
            }
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                if !is_value_type(*member_type) {
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
                Expr::binary(BinaryOp::Add, base, Expr::int(i64::from(*offset)), ty)
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
            Expression::Index { base, index } => self.pointer_sum(base, index),
            Expression::Dereference { pointer } => self.expression(pointer),
            other => Err(unsupported(format!("address of {}", expression_name(other)))),
        }
    }

    /// A call; `discarded` when its result is unused (a `void` callee is fine).
    /// A call through a function pointer: `mtctr` + `bctrl`. The callee's
    /// type is not tracked, so only a discarded result is modeled.
    fn indirect_call(&mut self, target: &Expression, arguments: &[Expression], discarded: bool) -> Compilation<Expr> {
        if !discarded {
            return Err(unsupported("value of an indirect call"));
        }
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
        Ok(Expr { kind: ExprKind::Call { name: mwcc_iro::INDIRECT_CALL.to_owned(), arguments: values }, ty: Type::Void })
    }

    fn call(&mut self, name: &str, arguments: &[Expression], discarded: bool) -> Compilation<Expr> {
        // A call through a pointer variable (local, parameter or global).
        let pointer_variable = self.names.contains_key(name)
            || self.unit.globals.get(name).is_some_and(|global| !global.is_function);
        if pointer_variable {
            return self.indirect_call(&Expression::Variable(name.to_owned()), arguments, discarded);
        }
        {
            {
                let ty = self.unit.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_value_type(ty) && !(discarded && ty == Type::Void) {
                    return Err(unsupported("non-integer call result"));
                }
                if (self.unit.is_intrinsic)(name, arguments.len()) {
                    return Err(unsupported(format!("intrinsic '{name}'")));
                }
                let variadic = self.unit.variadic_callees.contains(name);
                let prototyped = self.unit.prototyped.contains(name);
                if (self.unit.has_body)(name) {
                    return Err(unsupported("call to a function this unit defines (inlining not modeled)"));
                }
                if arguments.len() > ARGUMENT_REGISTERS {
                    return Err(unsupported("stack-passed arguments"));
                }
                let mut arguments = arguments.iter().map(|a| self.guarded_expression(a)).collect::<Compilation<Vec<_>>>()?;
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
                    // Default argument promotions; floating arguments of an
                    // unprototyped call also set CR1 (not modeled).
                    if arguments.iter().any(|argument| is_float(argument.ty)) {
                        return Err(unsupported("floating argument to a call without a prototype"));
                    }
                    let arguments = arguments.into_iter().map(promoted).collect();
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
                        } else if !is_narrow(parameter) {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = promoted(value);
                        }
                    }
                }
                if arguments.iter().any(|argument| !is_value_type(argument.ty)) {
                    return Err(unsupported("argument type"));
                }
                let floats = arguments.iter().filter(|argument| is_float(argument.ty)).count();
                if floats > ARGUMENT_REGISTERS || arguments.len() - floats > ARGUMENT_REGISTERS {
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
fn arithmetic_type(left: Type, right: Type) -> Type {
    let (left, right) = (promote(left), promote(right));
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
