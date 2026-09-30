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
    element_size, is_general_word, is_narrow, is_unsigned, pointee_type, promote, BinaryOp, Expr, ExprKind,
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
    if function.return_type != Type::Void && !is_general_word(function.return_type) {
        return Err(unsupported(format!("return type {:?}", function.return_type)));
    }
    let mut variables = Vec::new();
    for parameter in &function.parameters {
        if !is_general_word(parameter.parameter_type) {
            return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
        }
        variables.push(Variable {
            name: parameter.name.clone(),
            ty: parameter.parameter_type,
            kind: VariableKind::Parameter,
        });
    }
    if function.parameters.len() > ARGUMENT_REGISTERS {
        return Err(unsupported("stack-passed parameters"));
    }
    for local in &function.locals {
        if local.is_static || local.is_volatile || local.array_length.is_some() {
            return Err(unsupported("static, volatile, or array locals"));
        }
        if !is_general_word(local.declared_type) {
            return Err(unsupported(format!("local type {:?}", local.declared_type)));
        }
        variables.push(Variable { name: local.name.clone(), ty: local.declared_type, kind: VariableKind::Local });
    }
    let mut builder = Builder {
        unit,
        names: variables.iter().enumerate().map(|(id, variable)| (variable.name.clone(), id)).collect(),
        variables: &variables,
    };
    let mut body = Vec::new();
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            let variable = builder.names[&local.name];
            body.push(Stmt::Assign { variable, value: builder.expression(initializer)? });
        }
    }
    for statement in &function.statements {
        body.push(builder.statement(statement)?);
    }
    for guard in &function.guards {
        body.push(Stmt::If {
            condition: builder.expression(&guard.condition)?,
            then_body: vec![Stmt::Return(Some(builder.expression(&guard.value)?))],
            else_body: Vec::new(),
        });
    }
    if let Some(value) = &function.return_expression {
        // The final value falls through to the exit.
        body.push(Stmt::SetReturn(builder.expression(value)?));
    }
    let returns_through_variable = function.return_type != Type::Void
        && (!function.guards.is_empty()
            || has_return(&function.statements)
            || matches!(function.return_expression, Some(Expression::Conditional { .. })));
    Ok(Built {
        function: Function {
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
        Statement::If { then_body, else_body, .. } => has_return(then_body) || has_return(else_body),
        Statement::Loop { body, .. } => has_return(body),
        _ => false,
    })
}

struct Builder<'a> {
    unit: &'a Unit<'a>,
    names: HashMap<String, VarId>,
    variables: &'a [Variable],
}

impl Builder<'_> {
    fn statement(&mut self, statement: &Statement) -> Compilation<Stmt> {
        Ok(match statement {
            Statement::Assign { name, value } => {
                let Some(&variable) = self.names.get(name) else {
                    return Err(unsupported(format!("assignment to non-local '{name}'")));
                };
                Stmt::Assign { variable, value: self.expression(value)? }
            }
            Statement::Expression(Expression::Call { name, arguments }) => {
                Stmt::Eval(self.call(name, arguments, true)?)
            }
            Statement::Store { target, value } => {
                let (place, ty) = self.place(target)?;
                Stmt::Store { place, ty, value: self.expression(value)? }
            }
            Statement::If { condition, then_body, else_body } => Stmt::If {
                condition: self.expression(condition)?,
                then_body: then_body.iter().map(|s| self.statement(s)).collect::<Compilation<_>>()?,
                else_body: else_body.iter().map(|s| self.statement(s)).collect::<Compilation<_>>()?,
            },
            Statement::Return(value) => {
                Stmt::Return(value.as_ref().map(|value| self.expression(value)).transpose()?)
            }
            other => return Err(unsupported(format!("statement {:?}", std::mem::discriminant(other)))),
        })
    }

    /// A store target and the type it holds.
    fn place(&mut self, target: &Expression) -> Compilation<(Place, Type)> {
        match target {
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let base = self.expression(base)?;
                Ok((Place::Memory { base: Box::new(base), index: None, offset: *offset as i32 }, *member_type))
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
        if !matches!(base.ty, Type::Pointer(_)) {
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
        Ok(Some((Box::new(pointer), None, 0, loaded)))
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
                Ok(Expr::binary(op, left, scale(right, size), ty))
            }
            (None, Some(size)) if op == BinaryOp::Add => {
                let ty = right.ty;
                Ok(Expr::binary(op, scale(left, size), right, ty))
            }
            (None, Some(_)) => Err(unsupported("integer minus pointer")),
            (None, None) => unreachable!("pointer arithmetic needs a pointer"),
        }
    }

    fn expression(&mut self, expression: &Expression) -> Compilation<Expr> {
        Ok(match expression {
            Expression::IntegerLiteral(value) => Expr::int(*value),
            Expression::Variable(name) => {
                if let Some(&id) = self.names.get(name) {
                    return Ok(Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty });
                }
                let Some(global) = self.unit.globals.get(name) else {
                    return Err(unsupported(format!("unknown variable '{name}'")));
                };
                if global.is_array {
                    return Err(unsupported("array global as a value"));
                }
                Expr { kind: ExprKind::Global(name.clone()), ty: global.ty }
            }
            Expression::Binary { operator, left, right } => {
                let op = binary_op(*operator);
                let left = self.expression(left)?;
                let right = self.expression(right)?;
                if matches!(op, BinaryOp::Add | BinaryOp::Subtract)
                    && (element_size(left.ty).is_some() || element_size(right.ty).is_some())
                {
                    return self.pointer_arithmetic(op, left, right);
                }
                let ty = if op.is_comparison() || matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    Type::Int
                } else if matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight) {
                    promote(left.ty)
                } else {
                    arithmetic_type(left.ty, right.ty)
                };
                Expr::binary(op, left, right, ty)
            }
            Expression::Unary { operator, operand } => {
                let operand = self.expression(operand)?;
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
                let condition = self.expression(condition)?;
                let when_true = self.expression(when_true)?;
                let when_false = self.expression(when_false)?;
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
            Expression::Cast { target_type, operand } if is_general_word(*target_type) => {
                let operand = self.expression(operand)?;
                Expr { kind: ExprKind::Convert(Box::new(operand)), ty: *target_type }
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let base = self.expression(base)?;
                Expr { kind: ExprKind::Load { base: Box::new(base), index: None, offset: *offset as i32 }, ty: *member_type }
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
            _ => return Err(unsupported("expression form")),
        })
    }

    /// A call; `discarded` when its result is unused (a `void` callee is fine).
    fn call(&mut self, name: &str, arguments: &[Expression], discarded: bool) -> Compilation<Expr> {
        {
            {
                let ty = self.unit.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_general_word(ty) && !(discarded && ty == Type::Void) {
                    return Err(unsupported("non-integer call result"));
                }
                if (self.unit.is_intrinsic)(name, arguments.len()) {
                    return Err(unsupported(format!("intrinsic '{name}'")));
                }
                if self.unit.variadic_callees.contains(name) {
                    return Err(unsupported("call to a variadic function"));
                }
                if !self.unit.prototyped.contains(name) {
                    return Err(unsupported("call without a prototype"));
                }
                if self.unit.call_parameter_types.get(name).is_some_and(|types| types.iter().any(|ty| is_narrow(*ty))) {
                    return Err(unsupported("call with narrow parameters"));
                }
                if arguments.len() > ARGUMENT_REGISTERS {
                    return Err(unsupported("stack-passed arguments"));
                }
                let arguments = arguments.iter().map(|a| self.expression(a)).collect::<Compilation<Vec<_>>>()?;
                if arguments.iter().any(|argument| !is_general_word(argument.ty)) {
                    return Err(unsupported("non-integer argument"));
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
