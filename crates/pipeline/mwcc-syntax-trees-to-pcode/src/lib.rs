//! INITIAL CODE: lower a function's syntax tree to PCode over virtual
//! registers, as MWCC's `CodeGen_Generator` does before any backend pass.
//!
//! Virtual-register numbering follows MWCC's object-preallocation walk
//! (`CodeGen_PreallocateObjectRegisters`): parameters, then declared locals,
//! then the coalescing window opens and lowering temporaries follow in
//! creation order. Every unsupported construct rejects the whole function
//! with a diagnostic so the caller can fall back to another lowering.

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind, RelocationTarget};
use mwcc_pcode::{
    AttachedRelocation, Block, Class, PCodeFunction, PInstr, Register, ReturnRegisters,
};
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, Pointee, Statement, Type, UnaryOperator,
};

/// A file-scope object as lowering sees it.
#[derive(Debug, Clone, Copy)]
pub struct GlobalInfo {
    pub ty: Type,
    /// Addressed through the small-data base (`@sda21`) rather than `lis/@l`.
    pub small_data: bool,
    /// An array object: its name denotes its address, not a loadable value.
    pub is_array: bool,
    /// Every read and write is an observable access (never reused).
    pub is_volatile: bool,
}

/// Unit-level facts lowering needs.
pub struct LoweringContext<'a> {
    pub globals: &'a HashMap<String, GlobalInfo>,
    pub call_return_types: &'a HashMap<String, Type>,
    /// Calls the compiler expands inline (`__cntlzw`, `__sync`, ...).
    pub is_intrinsic: &'a dyn Fn(&str, usize) -> bool,
    /// Callees declared with `...` (the caller must set CR1 for them).
    pub variadic_callees: &'a std::collections::HashSet<String>,
    /// Callees with a prototype in scope.
    pub prototyped: &'a std::collections::HashSet<String>,
    /// Declared parameter types of callees.
    pub call_parameter_types: &'a HashMap<String, Vec<Type>>,
}

/// A lowered function.
#[derive(Debug, Clone)]
pub struct Lowered {
    pub pcode: PCodeFunction,
    /// The function calls another (it needs a frame and the saved LR).
    pub makes_calls: bool,
}

const FIRST_GENERAL_ARGUMENT: u32 = 3;
const LAST_GENERAL_ARGUMENT: u32 = 10;
const VOLATILE_GENERAL: [u32; 11] = [0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

fn unsupported(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("PCode lowering: {} (not yet supported)", what.into()))
}

/// Lower `function` to PCode.
pub fn lower(function: &Function, context: &LoweringContext<'_>) -> Compilation<Lowered> {
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return Err(unsupported("inline assembly"));
    }
    if !function.guards.is_empty() {
        return Err(unsupported("guarded returns"));
    }
    let returns = match function.return_type {
        Type::Void => ReturnRegisters::None,
        ty if is_general_word(ty) => ReturnRegisters::General,
        other => return Err(unsupported(format!("return type {other:?}"))),
    };
    let mut lowerer = Lowerer {
        context,
        pcode: PCodeFunction::new(function.name.clone(), returns),
        variables: HashMap::new(),
        makes_calls: false,
        return_type: function.return_type,
        target: None,
        loaded_globals: HashMap::new(),
    };
    lowerer.lower_function(function)?;
    Ok(Lowered { pcode: lowerer.pcode, makes_calls: lowerer.makes_calls })
}

fn is_general_word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::UnsignedInt
            | Type::Char
            | Type::UnsignedChar
            | Type::Short
            | Type::UnsignedShort
            | Type::Pointer(_)
            | Type::StructPointer { .. }
    )
}

fn is_unsigned(ty: Type) -> bool {
    matches!(
        ty,
        Type::UnsignedInt
            | Type::UnsignedChar
            | Type::UnsignedShort
            | Type::Pointer(_)
            | Type::StructPointer { .. }
    )
}

/// The C integer promotion of an operand type.
fn promote(ty: Type) -> Type {
    match ty {
        Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort => Type::Int,
        other => other,
    }
}

fn pointee_type(pointee: Pointee) -> Option<Type> {
    Some(match pointee {
        Pointee::Int => Type::Int,
        Pointee::UnsignedInt => Type::UnsignedInt,
        Pointee::Char => Type::Char,
        Pointee::UnsignedChar => Type::UnsignedChar,
        Pointee::Short => Type::Short,
        Pointee::UnsignedShort => Type::UnsignedShort,
        Pointee::Pointer | Pointee::WordPointer => Type::Pointer(Pointee::Int),
        _ => return None,
    })
}

struct Variable {
    register: u32,
    ty: Type,
}

struct Lowerer<'a> {
    context: &'a LoweringContext<'a>,
    pcode: PCodeFunction,
    variables: HashMap<String, Variable>,
    makes_calls: bool,
    return_type: Type,
    /// Destination requested for the next expression's final operation
    /// (an assignment computes straight into its variable's register).
    target: Option<u32>,
    /// Loaded values of non-volatile globals still valid in this statement
    /// (IRO common subexpressions); cleared by stores and calls.
    loaded_globals: HashMap<String, (u32, Type)>,
}

impl Lowerer<'_> {
    fn expression_with_target(
        &mut self,
        expression: &Expression,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        self.target = target;
        let evaluated = self.expression(expression);
        self.target = None;
        evaluated
    }

    fn result(&mut self, target: Option<u32>) -> u32 {
        target.unwrap_or_else(|| self.temporary())
    }

    fn emit(&mut self, instruction: PInstr) {
        let block = self.pcode.blocks.len() - 1;
        self.pcode.blocks[block].instructions.push(instruction);
    }

    fn emit_plain(&mut self, instruction: Instruction) {
        self.emit(PInstr::new(instruction));
    }

    /// An instruction whose `base` operand may not be r0.
    fn emit_based(&mut self, instruction: Instruction, base: u32) {
        let mut instruction = PInstr::new(instruction);
        instruction.not_r0.push(base);
        self.emit(instruction);
    }

    /// The type an expression evaluates to, where it is known without
    /// lowering it (enough to recognize pointer operands).
    fn static_type(&self, expression: &Expression) -> Option<Type> {
        match expression {
            Expression::Variable(name) => self
                .variables
                .get(name)
                .map(|variable| variable.ty)
                .or_else(|| {
                    self.context
                        .globals
                        .get(name)
                        .filter(|global| !global.is_array)
                        .map(|global| global.ty)
                }),
            Expression::Member { member_type, .. } => Some(*member_type),
            Expression::Cast { target_type, .. } => Some(*target_type),
            Expression::Call { name, .. } => self.context.call_return_types.get(name).copied(),
            Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right } => {
                let left = self.static_type(left);
                if left.is_some_and(|ty| element_size(ty).is_some()) {
                    return left;
                }
                self.static_type(right).filter(|ty| element_size(*ty).is_some())
            }
            _ => None,
        }
    }

    /// A parameter or local reference: already in a register.
    fn is_register_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name) if self.variables.contains_key(name))
    }

    fn temporary(&mut self) -> u32 {
        self.pcode.fresh(Class::General)
    }

    fn lower_function(&mut self, function: &Function) -> Compilation<()> {
        // Object preallocation: parameters, then locals in reverse declaration
        // order, then the coalescing window opens.
        let mut incoming = Vec::new();
        for (index, parameter) in function.parameters.iter().enumerate() {
            if !is_general_word(parameter.parameter_type) {
                return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
            }
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            if register > LAST_GENERAL_ARGUMENT {
                return Err(unsupported("stack-passed parameters"));
            }
            let virtual_register = self.temporary();
            self.variables.insert(
                parameter.name.clone(),
                Variable { register: virtual_register, ty: parameter.parameter_type },
            );
            incoming.push((virtual_register, register));
        }
        for local in function.locals.iter().rev() {
            if local.is_static || local.is_volatile || local.array_length.is_some() {
                return Err(unsupported("static, volatile, or array locals"));
            }
            if !is_general_word(local.declared_type) {
                return Err(unsupported(format!("local type {:?}", local.declared_type)));
            }
            let register = self.temporary();
            self.variables
                .insert(local.name.clone(), Variable { register, ty: local.declared_type });
        }
        self.pcode.begin_coalesce_window();

        for (virtual_register, register) in incoming {
            // Narrow parameters are re-extended by the callee on entry.
            let ty = self
                .variables
                .values()
                .find(|variable| variable.register == virtual_register)
                .map(|variable| variable.ty)
                .unwrap_or(Type::Int);
            let instruction = match ty {
                Type::Char => Instruction::ExtendSignByte { a: virtual_register, s: register },
                Type::Short => Instruction::ExtendSignHalfword { a: virtual_register, s: register },
                Type::UnsignedChar => {
                    Instruction::ClearLeftImmediate { a: virtual_register, s: register, clear: 24 }
                }
                Type::UnsignedShort => {
                    Instruction::ClearLeftImmediate { a: virtual_register, s: register, clear: 16 }
                }
                _ => Instruction::Or { a: virtual_register, s: register, b: register },
            };
            self.emit_plain(instruction);
        }
        for local in &function.locals {
            if let Some(initializer) = &local.initializer {
                self.assign(&local.name, initializer)?;
            }
        }
        for statement in &function.statements {
            self.statement(statement)?;
        }
        if let Some(value) = &function.return_expression {
            self.return_value(value)?;
        }
        // The exit block: the epilogue is generated here after coloring.
        self.pcode.blocks.last_mut().expect("entry block").successors.push(1);
        self.pcode.blocks.push(Block { weight: 1, ..Block::default() });
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Assign { name, value } => self.assign(name, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.call(name, arguments, None).map(|_| ())
            }
            Statement::Store { target, value } => self.store(target, value),
            other => Err(unsupported(format!("statement {:?}", std::mem::discriminant(other)))),
        }
    }

    fn return_value(&mut self, value: &Expression) -> Compilation<()> {
        let (register, ty) = self.expression(value)?;
        let (register, _) = self.convert(register, ty, self.return_type, None)?;
        self.emit_plain(Instruction::Or { a: 3, s: register, b: register });
        Ok(())
    }

    fn assign(&mut self, name: &str, value: &Expression) -> Compilation<()> {
        let Some(variable) = self.variables.get(name) else {
            return Err(unsupported(format!("assignment to non-local '{name}'")));
        };
        let destination = variable.register;
        let result = self.assign_to(name, destination, value);
        // A cached global value held in this variable's register is stale
        // once the variable changes (the new value may itself be cached).
        let _ = name;
        result
    }

    fn assign_to(&mut self, name: &str, destination: u32, value: &Expression) -> Compilation<()> {
        let before: Vec<String> = self
            .loaded_globals
            .iter()
            .filter(|(_, (register, _))| *register == destination)
            .map(|(global, _)| global.clone())
            .collect();
        for global in before {
            self.loaded_globals.remove(&global);
        }
        let Some(variable) = self.variables.get(name) else {
            return Err(unsupported(format!("assignment to non-local '{name}'")));
        };
        let variable_type = variable.ty;
        let narrow = matches!(
            variable_type,
            Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort
        );
        self.target = if narrow { None } else { Some(destination) };
        let evaluated = self.expression(value);
        self.target = None;
        let (source, source_type) = evaluated?;
        if narrow && source_type != variable_type {
            let (converted, _) = self.convert(source, source_type, variable_type, Some(destination))?;
            if converted == destination {
                return Ok(());
            }
            self.emit_plain(Instruction::Or { a: destination, s: converted, b: converted });
            return Ok(());
        }
        if source != destination {
            self.emit_plain(Instruction::Or { a: destination, s: source, b: source });
        }
        Ok(())
    }

    /// Evaluate a word-sized integer/pointer expression into a register.
    fn expression(&mut self, expression: &Expression) -> Compilation<(u32, Type)> {
        let target = self.target.take();
        match expression {
            Expression::IntegerLiteral(value) => {
                let register = self.result(target);
                self.load_constant(register, *value)?;
                Ok((register, Type::Int))
            }
            Expression::Variable(name) => {
                if let Some(variable) = self.variables.get(name) {
                    return Ok((variable.register, variable.ty));
                }
                if let Some(global) = self.context.globals.get(name).copied() {
                    if global.is_array {
                        return Err(unsupported("array global as a value"));
                    }
                    if !global.is_volatile {
                        if let Some(&(register, ty)) = self.loaded_globals.get(name) {
                            return Ok((register, ty));
                        }
                    }
                    let loaded = self.load_global(name, global, target)?;
                    if !global.is_volatile {
                        self.loaded_globals.insert(name.clone(), loaded);
                    }
                    return Ok(loaded);
                }
                Err(unsupported(format!("unknown variable '{name}'")))
            }
            Expression::Binary { operator, left, right } => {
                match fold_constants(*operator, left, right) {
                    Some(Expression::Binary { operator, left, right }) => {
                        self.binary(operator, &left, &right, target)
                    }
                    _ => self.binary(*operator, left, right, target),
                }
            }
            Expression::Unary { operator, operand } => {
                let (source, ty) = self.expression(operand)?;
                let destination = self.result(target);
                match operator {
                    UnaryOperator::Negate => {
                        self.emit_plain(Instruction::Negate { d: destination, a: source })
                    }
                    UnaryOperator::BitNot => {
                        self.emit_plain(Instruction::Nor { a: destination, s: source, b: source })
                    }
                    UnaryOperator::LogicalNot => return Err(unsupported("logical not")),
                }
                Ok((destination, promote(ty)))
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let (base, _) = self.expression(base)?;
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                self.load(*member_type, base, offset, None, target)
            }
            Expression::Dereference { pointer } => {
                let (base, ty) = self.expression(pointer)?;
                let Type::Pointer(pointee) = ty else {
                    return Err(unsupported("dereference of a non-scalar pointer"));
                };
                let loaded = pointee_type(pointee).ok_or_else(|| unsupported("pointee type"))?;
                self.load(loaded, base, 0, None, target)
            }
            Expression::Call { name, arguments } => {
                let ty = self.context.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_general_word(ty) {
                    return Err(unsupported("non-integer call result"));
                }
                let register = self.call(name, arguments, target)?;
                Ok((register, ty))
            }
            Expression::Cast { target_type, operand } if is_general_word(*target_type) => {
                let (source, source_type) = self.expression(operand)?;
                self.convert(source, source_type, *target_type, target)
            }
            _ => Err(unsupported("expression form")),
        }
    }

    fn convert(
        &mut self,
        source: u32,
        from: Type,
        to: Type,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        let narrow = match to {
            Type::Char => Some(Instruction::ExtendSignByte { a: 0, s: source }),
            Type::Short => Some(Instruction::ExtendSignHalfword { a: 0, s: source }),
            Type::UnsignedChar => Some(Instruction::ClearLeftImmediate { a: 0, s: source, clear: 24 }),
            Type::UnsignedShort => Some(Instruction::ClearLeftImmediate { a: 0, s: source, clear: 16 }),
            _ => None,
        };
        if from == to || narrow.is_none() {
            return Ok((source, to));
        }
        let destination = self.result(target);
        let instruction = match narrow.expect("checked") {
            Instruction::ExtendSignByte { s, .. } => Instruction::ExtendSignByte { a: destination, s },
            Instruction::ExtendSignHalfword { s, .. } => Instruction::ExtendSignHalfword { a: destination, s },
            Instruction::ClearLeftImmediate { s, clear, .. } => {
                Instruction::ClearLeftImmediate { a: destination, s, clear }
            }
            other => other,
        };
        self.emit_plain(instruction);
        Ok((destination, to))
    }

    fn load_constant(&mut self, register: u32, value: i64) -> Compilation<()> {
        let value = i32::try_from(value)
            .or_else(|_| u32::try_from(value).map(|value| value as i32))
            .map_err(|_| unsupported("64-bit constant"))?;
        if let Ok(small) = i16::try_from(value) {
            self.emit_plain(Instruction::AddImmediate { d: register, a: 0, immediate: small });
            return Ok(());
        }
        let low = value as i16;
        let high = ((value - i32::from(low)) >> 16) as i16;
        self.emit_plain(Instruction::AddImmediateShifted { d: register, a: 0, immediate: high });
        if low != 0 {
            self.emit_based(Instruction::AddImmediate { d: register, a: register, immediate: low }, register);
        }
        Ok(())
    }

    fn load_instruction(ty: Type, d: u32, a: u32, offset: i16) -> Compilation<(Instruction, Option<Instruction>)> {
        Ok(match ty {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                (Instruction::LoadWord { d, a, offset }, None)
            }
            Type::Short => (Instruction::LoadHalfwordAlgebraic { d, a, offset }, None),
            Type::UnsignedShort => (Instruction::LoadHalfwordZero { d, a, offset }, None),
            Type::UnsignedChar => (Instruction::LoadByteZero { d, a, offset }, None),
            Type::Char => (
                Instruction::LoadByteZero { d, a, offset },
                Some(Instruction::ExtendSignByte { a: d, s: d }),
            ),
            other => return Err(unsupported(format!("load of {other:?}"))),
        })
    }

    fn load(
        &mut self,
        ty: Type,
        base: u32,
        offset: i16,
        relocation: Option<AttachedRelocation>,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        let extends = matches!(ty, Type::Char);
        let destination = if extends { self.temporary() } else { self.result(target) };
        let (load, extend) = Self::load_instruction(ty, destination, base, offset)?;
        let mut instruction = PInstr::new(load);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        if let Some(extend) = extend {
            let extended = self.result(target);
            let extend = match extend {
                Instruction::ExtendSignByte { .. } => Instruction::ExtendSignByte { a: extended, s: destination },
                other => other,
            };
            self.emit_plain(extend);
            return Ok((extended, ty));
        }
        Ok((destination, ty))
    }

    fn load_global(
        &mut self,
        name: &str,
        global: GlobalInfo,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        if global.small_data {
            return self.load(
                global.ty,
                0,
                0,
                Some(AttachedRelocation {
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::External(name.to_owned()),
                }),
                target,
            );
        }
        let high = self.temporary();
        let mut lis = PInstr::new(Instruction::AddImmediateShifted { d: high, a: 0, immediate: 0 });
        lis.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(name.to_owned()),
        });
        self.emit(lis);
        self.load(
            global.ty,
            high,
            0,
            Some(AttachedRelocation {
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External(name.to_owned()),
            }),
            target,
        )
    }

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        target: Option<u32>,
    ) -> Compilation<(u32, Type)> {
        if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
            let left_pointer = self.static_type(left).and_then(element_size);
            let right_pointer = self.static_type(right).and_then(element_size);
            match (left_pointer, right_pointer) {
                (Some(_), Some(_)) => return Err(unsupported("pointer difference")),
                (Some(0), None) | (None, Some(0)) => {
                    return Err(unsupported("arithmetic on an unsized pointee"))
                }
                (Some(size), None) if size != 1 => {
                    let scaled = scale_index(right, size);
                    return self.binary(operator, left, &scaled, target);
                }
                (None, Some(size)) if size != 1 && operator == BinaryOperator::Add => {
                    let scaled = scale_index(left, size);
                    return self.binary(operator, &scaled, right, target);
                }
                (None, Some(_)) if operator == BinaryOperator::Subtract => {
                    return Err(unsupported("integer minus pointer"))
                }
                _ => {}
            }
        }
        let immediate = match right {
            Expression::IntegerLiteral(value) => i16::try_from(*value).ok(),
            _ => None,
        };
        // IRO algebra: `(x + y) - y` and `(x - y) + y` are `x`.
        if let Some(simplified) = cancel(operator, left, right) {
            return self.expression_with_target(simplified, target);
        }
        // `a & ~b` / `a | ~b` are single instructions.
        if let (BinaryOperator::BitAnd | BinaryOperator::BitOr, Expression::Unary { operator: UnaryOperator::BitNot, operand }) =
            (operator, right)
        {
            let (a, ty) = self.expression(left)?;
            let (b, _) = self.expression(operand)?;
            let d = self.result(target);
            self.emit_plain(if operator == BinaryOperator::BitAnd {
                Instruction::AndComplement { a: d, s: a, b }
            } else {
                Instruction::OrComplement { a: d, s: a, b }
            });
            return Ok((d, promote(ty)));
        }
        // A mask within a narrow load's width does not observe its sign
        // extension: load zero-extended and mask.
        if operator == BinaryOperator::BitAnd {
            if let (Some((begin, end)), Some(width)) = (mask_bounds(right), narrow_load_width(left)) {
                if begin >= 32 - width {
                    let widened = zero_extended_load(left);
                    let (a, _) = self.expression(&widened)?;
                    let d = self.result(target);
                    self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                    return Ok((d, Type::Int));
                }
            }
        }
        if operator == BinaryOperator::BitAnd {
            if let Some((operand, shift, begin, end)) = shift_mask(left, right) {
                let (a, operand_type) = self.expression(operand)?;
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift, begin, end });
                return Ok((d, promote(operand_type)));
            }
        }
        let (a, left_type) = self.expression(left)?;
        let result_type = promote(left_type);
        match (operator, immediate) {
            (BinaryOperator::Add, Some(value)) => {
                let d = self.result(target);
                self.emit_based(Instruction::AddImmediate { d, a, immediate: value }, a);
                return Ok((d, result_type));
            }
            (BinaryOperator::Add, None) if wide_constant(right).is_some() => {
                let (high, low) = wide_constant(right).expect("checked");
                let d = self.result(target);
                let middle = if low == 0 { d } else { self.temporary() };
                self.emit_based(Instruction::AddImmediateShifted { d: middle, a, immediate: high }, a);
                if low != 0 {
                    self.emit_based(Instruction::AddImmediate { d, a: middle, immediate: low }, middle);
                }
                return Ok((d, result_type));
            }
            (BinaryOperator::Subtract, Some(value)) if value != i16::MIN => {
                let d = self.result(target);
                self.emit_based(Instruction::AddImmediate { d, a, immediate: -value }, a);
                return Ok((d, result_type));
            }
            (BinaryOperator::Multiply, Some(value)) if value > 0 && (value as u16).is_power_of_two() => {
                let d = self.result(target);
                let shift = (value as u16).trailing_zeros() as u8;
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift });
                return Ok((d, result_type));
            }
            (BinaryOperator::Multiply, Some(value)) => {
                let d = self.result(target);
                self.emit_plain(Instruction::MultiplyImmediate { d, a, immediate: value });
                return Ok((d, result_type));
            }
            (BinaryOperator::BitOr | BinaryOperator::BitXor, _) if unsigned_immediate(right).is_some() => {
                let (value, shifted) = unsigned_immediate(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(match (operator, shifted) {
                    (BinaryOperator::BitOr, false) => Instruction::OrImmediate { a: d, s: a, immediate: value },
                    (BinaryOperator::BitOr, true) => Instruction::OrImmediateShifted { a: d, s: a, immediate: value },
                    (_, false) => Instruction::XorImmediate { a: d, s: a, immediate: value },
                    (_, true) => Instruction::XorImmediateShifted { a: d, s: a, immediate: value },
                });
                return Ok((d, result_type));
            }
            (BinaryOperator::BitAnd, _) if mask_bounds(right).is_some() => {
                let (begin, end) = mask_bounds(right).expect("checked");
                let d = self.result(target);
                self.emit_plain(Instruction::RotateAndMask { a: d, s: a, shift: 0, begin, end });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftLeft, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.result(target);
                self.emit_plain(Instruction::ShiftLeftImmediate { a: d, s: a, shift: shift as u8 });
                return Ok((d, result_type));
            }
            (BinaryOperator::ShiftRight, Some(shift)) if (0..32).contains(&shift) => {
                let d = self.result(target);
                let shift = shift as u8;
                self.emit_plain(if is_unsigned(result_type) {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: a, shift }
                } else {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: a, shift }
                });
                return Ok((d, result_type));
            }
            _ => {}
        }
        let (b, right_type) = self.expression(right)?;
        let result_type = if is_unsigned(result_type) || is_unsigned(promote(right_type)) {
            if matches!(result_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                result_type
            } else {
                Type::UnsignedInt
            }
        } else {
            result_type
        };
        // MWCC places a leaf operand first in a commutative operation whose
        // other operand is computed (`a*b + c` -> `add r3,c,t`).
        let commutative = matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Multiply
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor
        );
        let (a, b) = if commutative && !self.is_register_leaf(left) && self.is_register_leaf(right) {
            (b, a)
        } else {
            (a, b)
        };
        let d = self.result(target);
        let instruction = match operator {
            BinaryOperator::Add => Instruction::Add { d, a, b },
            BinaryOperator::Subtract => Instruction::SubtractFrom { d, a: b, b: a },
            BinaryOperator::Multiply => Instruction::MultiplyLow { d, a, b },
            BinaryOperator::BitAnd => Instruction::And { a: d, s: a, b },
            BinaryOperator::BitOr => Instruction::Or { a: d, s: a, b },
            BinaryOperator::BitXor => Instruction::Xor { a: d, s: a, b },
            BinaryOperator::ShiftLeft => Instruction::ShiftLeftWord { a: d, s: a, b },
            BinaryOperator::ShiftRight if is_unsigned(result_type) => {
                Instruction::ShiftRightWord { a: d, s: a, b }
            }
            BinaryOperator::ShiftRight => Instruction::ShiftRightAlgebraicWord { a: d, s: a, b },
            other => return Err(unsupported(format!("binary operator {other:?}"))),
        };
        self.emit_plain(instruction);
        Ok((d, result_type))
    }

    fn store(&mut self, target: &Expression, value: &Expression) -> Compilation<()> {
        let width = store_width(target, self.context.globals);
        let value = strip_narrowing_for_store(value, width);
        let (source, _) = self.expression(value)?;
        let (base, offset, ty, relocation) = match target {
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let (base, _) = self.expression(base)?;
                let offset = i16::try_from(*offset).map_err(|_| unsupported("large member offset"))?;
                (base, offset, *member_type, None)
            }
            Expression::Dereference { pointer } => {
                let (base, ty) = self.expression(pointer)?;
                let Type::Pointer(pointee) = ty else {
                    return Err(unsupported("store through a non-scalar pointer"));
                };
                (base, 0, pointee_type(pointee).ok_or_else(|| unsupported("pointee type"))?, None)
            }
            Expression::Variable(name) if self.context.globals.contains_key(name) => {
                let global = self.context.globals[name];
                if global.is_array {
                    return Err(unsupported("store to an array global"));
                }
                if !global.small_data {
                    return Err(unsupported("absolute global store"));
                }
                (
                    0,
                    0,
                    global.ty,
                    Some(AttachedRelocation {
                        kind: RelocationKind::EmbSda21,
                        target: RelocationTarget::External(name.clone()),
                    }),
                )
            }
            _ => return Err(unsupported("store target")),
        };
        let store = match ty {
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. } => {
                Instruction::StoreWord { s: source, a: base, offset }
            }
            Type::Short | Type::UnsignedShort => Instruction::StoreHalfword { s: source, a: base, offset },
            Type::Char | Type::UnsignedChar => Instruction::StoreByte { s: source, a: base, offset },
            other => return Err(unsupported(format!("store of {other:?}"))),
        };
        let mut instruction = PInstr::new(store);
        if base != 0 {
            instruction.not_r0.push(base);
        }
        instruction.relocation = relocation;
        self.emit(instruction);
        self.loaded_globals.clear();
        // A stored word-sized global's value is still in `source`.
        if let Expression::Variable(name) = target {
            if let Some(global) = self.context.globals.get(name) {
                if !global.is_volatile && matches!(ty, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }) {
                    self.loaded_globals.insert(name.clone(), (source, ty));
                }
            }
        }
        Ok(())
    }

    fn call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        target: Option<u32>,
    ) -> Compilation<u32> {
        if (self.context.is_intrinsic)(name, arguments.len()) {
            return Err(unsupported(format!("intrinsic '{name}'")));
        }
        if self.context.variadic_callees.contains(name) {
            return Err(unsupported("call to a variadic function"));
        }
        if !self.context.prototyped.contains(name) {
            return Err(unsupported("call without a prototype"));
        }
        if self.context.call_parameter_types.get(name).is_some_and(|types| {
            types.iter().any(|ty| {
                matches!(ty, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
            })
        }) {
            return Err(unsupported("call with narrow parameters"));
        }
        if arguments.len() > (LAST_GENERAL_ARGUMENT - FIRST_GENERAL_ARGUMENT + 1) as usize {
            return Err(unsupported("stack-passed arguments"));
        }
        let mut values = Vec::new();
        for argument in arguments {
            let (register, ty) = self.expression(argument)?;
            if !is_general_word(ty) {
                return Err(unsupported("non-integer argument"));
            }
            values.push(register);
        }
        let mut argument_registers = Vec::new();
        for (index, value) in values.into_iter().enumerate() {
            let register = FIRST_GENERAL_ARGUMENT + index as u32;
            self.emit_plain(Instruction::Or { a: register, s: value, b: value });
            argument_registers.push(Register::general(register));
        }
        let mut call = PInstr::new(Instruction::BranchAndLink { target: name.to_owned() });
        call.relocation = Some(AttachedRelocation {
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(name.to_owned()),
        });
        call.implicit_uses = argument_registers;
        call.implicit_defs = VOLATILE_GENERAL.iter().map(|&r| Register::general(r)).collect();
        call.implicit_defs.extend((0..14).map(Register::float));
        self.emit(call);
        self.makes_calls = true;
        self.loaded_globals.clear();
        let result = self.result(target);
        self.emit_plain(Instruction::Or { a: result, s: 3, b: 3 });
        Ok(result)
    }
}

/// `x | k` / `x ^ k` immediates: a low halfword (`ori`) or a high halfword
/// with a zero low half (`oris`).
fn unsigned_immediate(expression: &Expression) -> Option<(u16, bool)> {
    let Expression::IntegerLiteral(value) = expression else { return None };
    let value = u32::try_from(*value).ok().or_else(|| i32::try_from(*value).ok().map(|v| v as u32))?;
    if value <= 0xFFFF {
        Some((value as u16, false))
    } else if value & 0xFFFF == 0 {
        Some(((value >> 16) as u16, true))
    } else {
        None
    }
}

/// A contiguous (possibly wrapping) mask `x & k` as `rlwinm` bounds.
fn mask_bounds(expression: &Expression) -> Option<(u8, u8)> {
    let Expression::IntegerLiteral(value) = expression else { return None };
    let mask = u32::try_from(*value).ok().or_else(|| i32::try_from(*value).ok().map(|v| v as u32))?;
    if mask == 0 || mask == u32::MAX {
        return None;
    }
    // MB..ME in IBM bit numbering (bit 0 = MSB). A non-wrapping run.
    let leading = mask.leading_zeros();
    let trailing = mask.trailing_zeros();
    if (mask >> trailing).count_ones() == 32 - leading - trailing {
        return Some((leading as u8, (31 - trailing) as u8));
    }
    None
}

/// IRO constant folding of a constant operation applied to another constant
/// operation of the same kind: `(x >> 2) >> 3` -> `x >> 5`,
/// `(x + 3) + 5` -> `x + 8`, `x + 10 - 3` -> `x + 7`, `(x | 5) | 2` -> `x | 7`.
fn fold_constants(
    operator: BinaryOperator,
    left: &Expression,
    right: &Expression,
) -> Option<Expression> {
    let Expression::IntegerLiteral(outer) = right else { return None };
    let Expression::Binary { operator: inner_operator, left: inner_left, right: inner_right } = left
    else {
        return None;
    };
    let Expression::IntegerLiteral(inner) = inner_right.as_ref() else { return None };
    use BinaryOperator::*;
    let (operator, value) = match (*inner_operator, operator) {
        (ShiftRight, ShiftRight) | (ShiftLeft, ShiftLeft) if inner + outer < 32 => {
            (operator, inner + outer)
        }
        (Add, Add) => (Add, inner + outer),
        (Add, Subtract) => (Add, inner - outer),
        (Subtract, Add) => (Add, outer - inner),
        (Subtract, Subtract) => (Subtract, inner + outer),
        (BitOr, BitOr) => (BitOr, inner | outer),
        (BitXor, BitXor) => (BitXor, inner ^ outer),
        (BitAnd, BitAnd) => (BitAnd, inner & outer),
        (Multiply, Multiply) => (Multiply, inner * outer),
        _ => return None,
    };
    let folded = Expression::Binary {
        operator,
        left: inner_left.clone(),
        right: Box::new(Expression::IntegerLiteral(value)),
    };
    // Fold repeatedly (`a + 1 + 2 + 3`).
    if let Expression::Binary { operator, left, right } = &folded {
        if let Some(again) = fold_constants(*operator, left, right) {
            return Some(again);
        }
    }
    Some(folded)
}

/// `(x >> s) & m` / `(x << s) & m` as one `rlwinm x, rotate, mb, me` when the
/// mask keeps only bits the shift defines: `(operand, rotate, mb, me)`.
fn shift_mask<'a>(left: &'a Expression, right: &Expression) -> Option<(&'a Expression, u8, u8, u8)> {
    let (begin, end) = mask_bounds(right)?;
    let Expression::IntegerLiteral(mask) = right else { return None };
    let mask = *mask as u32;
    let Expression::Binary { operator, left: operand, right: amount } = left else { return None };
    let Expression::IntegerLiteral(amount) = amount.as_ref() else { return None };
    let amount = u32::try_from(*amount).ok().filter(|amount| (1..32).contains(amount))?;
    match operator {
        BinaryOperator::ShiftRight if mask <= (u32::MAX >> amount) => {
            Some((operand, (32 - amount) as u8, begin, end))
        }
        BinaryOperator::ShiftLeft if mask & ((1 << amount) - 1) == 0 => {
            Some((operand, amount as u8, begin, end))
        }
        _ => None,
    }
}

/// Byte width of a store target, when known.
fn store_width(target: &Expression, globals: &HashMap<String, GlobalInfo>) -> Option<u32> {
    let ty = match target {
        Expression::Member { member_type, .. } => *member_type,
        Expression::Dereference { pointer } => match pointer.as_ref() {
            Expression::Cast { target_type: Type::Pointer(pointee), .. } => pointee_type(*pointee)?,
            _ => return None,
        },
        Expression::Variable(name) => globals.get(name)?.ty,
        _ => return None,
    };
    Some(match ty {
        Type::Char | Type::UnsignedChar => 1,
        Type::Short | Type::UnsignedShort => 2,
        _ => 4,
    })
}

/// A store keeps only its low `width` bytes, so integer conversions to types
/// at least that wide before it are dead.
fn strip_narrowing_for_store(value: &Expression, width: Option<u32>) -> &Expression {
    let Some(width) = width else { return value };
    let mut value = value;
    while let Expression::Cast { target_type, operand } = value {
        let cast_width = match target_type {
            Type::Char | Type::UnsignedChar => 1,
            Type::Short | Type::UnsignedShort => 2,
            Type::Int | Type::UnsignedInt => 4,
            _ => return value,
        };
        if cast_width < width {
            return value;
        }
        value = operand;
    }
    value
}

/// The byte size of a pointer operand's element, `Some(0)` when unknown
/// (opaque struct or function pointer), `None` for a non-pointer.
fn element_size(ty: Type) -> Option<u32> {
    match ty {
        Type::Pointer(pointee) => Some(match pointee {
            Pointee::Char | Pointee::UnsignedChar => 1,
            Pointee::Short | Pointee::UnsignedShort => 2,
            Pointee::Int
            | Pointee::UnsignedInt
            | Pointee::Float
            | Pointee::Pointer
            | Pointee::WordPointer => 4,
            Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong => 8,
        }),
        Type::StructPointer { element_size } => Some(element_size),
        _ => None,
    }
}

/// `index * size` for pointer arithmetic, folded when the index is constant.
fn scale_index(index: &Expression, size: u32) -> Expression {
    match index {
        Expression::IntegerLiteral(value) => Expression::IntegerLiteral(value * i64::from(size)),
        other => Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(other.clone()),
            right: Box::new(Expression::IntegerLiteral(i64::from(size))),
        },
    }
}

/// `(x + y) - y` / `(x - y) + y` -> `x` (matching leaf operands only).
fn cancel<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &Expression,
) -> Option<&'a Expression> {
    let Expression::Binary { operator: inner, left: x, right: y } = left else { return None };
    let leaf = |expression: &Expression| {
        matches!(expression, Expression::Variable(_) | Expression::IntegerLiteral(_))
    };
    let inverse = matches!(
        (inner, operator),
        (BinaryOperator::Add, BinaryOperator::Subtract) | (BinaryOperator::Subtract, BinaryOperator::Add)
    );
    let same = match (y.as_ref(), right) {
        (Expression::Variable(a), Expression::Variable(b)) => a == b,
        (Expression::IntegerLiteral(a), Expression::IntegerLiteral(b)) => a == b,
        _ => false,
    };
    (inverse && leaf(y) && same).then_some(x.as_ref())
}

/// A constant that does not fit `addi`: `(high, low)` for `addis`+`addi`.
fn wide_constant(expression: &Expression) -> Option<(i16, i16)> {
    let Expression::IntegerLiteral(value) = expression else { return None };
    let value = i32::try_from(*value).ok().or_else(|| u32::try_from(*value).ok().map(|v| v as i32))?;
    if i16::try_from(value).is_ok() {
        return None;
    }
    let low = value as i16;
    let high = ((value - i32::from(low)) >> 16) as i16;
    Some((high, low))
}

/// Bit width of a signed narrow memory load (`char`/`short` member or
/// dereference), whose sign extension a narrow mask never observes.
fn narrow_load_width(expression: &Expression) -> Option<u8> {
    match expression {
        Expression::Member { member_type: Type::Char, index_stride: None, .. } => Some(8),
        Expression::Member { member_type: Type::Short, index_stride: None, .. } => Some(16),
        _ => None,
    }
}

/// The same load, zero-extending.
fn zero_extended_load(expression: &Expression) -> Expression {
    match expression {
        Expression::Member { base, offset, member_type, index_stride } => Expression::Member {
            base: base.clone(),
            offset: *offset,
            member_type: match member_type {
                Type::Char => Type::UnsignedChar,
                Type::Short => Type::UnsignedShort,
                other => *other,
            },
            index_stride: *index_stride,
        },
        other => other.clone(),
    }
}
